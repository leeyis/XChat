package com.xchat.app

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.media.MediaRecorder
import android.net.Uri
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.provider.OpenableColumns
import android.provider.DocumentsContract
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.addCallback
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.Keep
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.net.Inet4Address
import java.net.NetworkInterface

/**
 * 和 ActivityResultContracts.TakePicture 唯一的区别：显式给输出 URI 加上写权限。
 *
 * 相机 App 是另一个进程，要通过 FileProvider 往我们的私有目录写照片，
 * 就必须拿到 FLAG_GRANT_WRITE_URI_PERMISSION；基类只 putExtra(EXTRA_OUTPUT)，
 * 不自己加这个 flag（能不能写全靠系统在 startActivity 时空口迁移 EXTRA_OUTPUT）。
 * 这里补上，免得在部分 ROM 上拍完拿不到文件。
 */
private class GrantWriteTakePicture : ActivityResultContracts.TakePicture() {
    override fun createIntent(context: Context, input: Uri): Intent =
        super.createIntent(context, input).addFlags(
            Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION
        )
}

// 通知 Intent 的 Key 常量（来自 tauri-plugin-notification）
private const val NOTIFICATION_INTENT_KEY = "NotificationId"
private const val NOTIFICATION_OBJ_INTENT_KEY = "LocalNotficationObject"
private const val ACTION_INTENT_KEY = "NotificationUserAction"

class MainActivity : TauriActivity() {
    // ─── JNI：Rust 侧的回调 ───
    private external fun nativeOnSafFileSelected(uri: String, name: String, size: Long)

    private var pendingSharedFiles: List<SharedFileInfo>? = null
    private var webView: WebView? = null
    private var shareReceiver: BroadcastReceiver? = null
    private var lastNotificationFromId: String? = null
    private var discoveryMulticastLock: WifiManager.MulticastLock? = null

    // ─── SAF 文件选择器（持久化权限） ───
    private val safPickerLauncher = registerForActivityResult(
        ActivityResultContracts.OpenDocument()
    ) { uri: Uri? ->
        if (uri != null) {
            stageSelectedAttachment(uri)
        }
    }

    // ─── 输入区扩展：拍照 / 录音 ───
    // 相机结果要经过用户交互（异步），因此按 injection 方式回传前端；
    // 录音的开始/结束是同步的，直接由 JNI 把 JSON 状态返回给 Rust 命令。
    private var pendingCameraFile: File? = null
    private var mediaRecorder: MediaRecorder? = null
    private var recordingFile: File? = null
    private var recordingStartedAt: Long = 0L

    private val cameraLauncher = registerForActivityResult(
        GrantWriteTakePicture()
    ) { success: Boolean ->
        handleCameraResult(success)
    }

    private val cameraPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted: Boolean ->
        if (granted) {
            launchCameraCaptureInternal()
        } else {
            println("[MainActivity] 相机权限被拒绝")
            pushCameraResult(JSONObject().put("status", "permission_denied"))
        }
    }

    private val audioPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted: Boolean ->
        // 权限是在「按住说话」时申请的，弹出系统弹窗时手指通常已经抬起，
        // 这里只记录结果：前端收到 permission_denied 已经提示用户重新按住。
        println("[MainActivity] 麦克风权限结果: $granted")
    }

    data class SharedFileInfo(
        val uri: Uri,
        val fileName: String,
        val fileSize: Long,
        val mimeType: String?
    )

    override fun onCreate(savedInstanceState: Bundle?) {
        // Bind before Rust creates its discovery and HTTP sockets.
        runCatching { getLanNetworkInterfaces() }
            .onFailure { println("[LAN] Initial network selection failed: ${it.message}") }
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        // WebView safe-area env() is zero on several Android versions. Inset the
        // native content instead, including cutouts, gesture navigation and IME.
        val content = findViewById<android.view.View>(android.R.id.content)
        ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
            val safe = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime())
            view.setPadding(safe.left, safe.top, safe.right, maxOf(safe.bottom, keyboard.bottom))
            WindowInsetsCompat.CONSUMED
        }
        ViewCompat.requestApplyInsets(content)

        // 系统返回键先进前端：在会话里应当回到会话列表，而不是直接把 App 退掉。
        // 前端用 window.__xchatHandleBack() 回答「我处理了没有」，
        // 返回 "true" 表示已消化这次返回，否则交回系统默认行为（退出）。
        onBackPressedDispatcher.addCallback(this) {
            val view = webView ?: findWebView(window.decorView)?.also { webView = it }
            if (view == null) {
                isEnabled = false
                onBackPressedDispatcher.onBackPressed()
                return@addCallback
            }
            view.evaluateJavascript(
                "(window.__xchatHandleBack && window.__xchatHandleBack()) === true"
            ) { result ->
                if (result != "true") {
                    // 前端无事可做，恢复默认行为再触发一次，让系统正常退出
                    isEnabled = false
                    onBackPressedDispatcher.onBackPressed()
                }
            }
        }

        // 开启 WebView 调试（方便 adb logcat 看到 JS console 输出）
        android.webkit.WebView.setWebContentsDebuggingEnabled(true)
        
        // 注册广播接收器
        registerShareReceiver()
        
        // 检测冷启动是否来自通知点击
        checkNotificationLaunch(intent)
    }

    override fun onStart() {
        super.onStart()
        acquireDiscoveryMulticastLock()
    }

    /** Called by the Rust inventory poll; never derive LAN addresses from the default VPN route. */
    @Keep
    @Synchronized
    fun getLanNetworkInterfaces(): String {
        val manager = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val candidates = manager.allNetworks.mapNotNull { network ->
            val capabilities = manager.getNetworkCapabilities(network) ?: return@mapNotNull null
            if (!capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN) ||
                capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN) ||
                !(capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET))) return@mapNotNull null
            val links = manager.getLinkProperties(network) ?: return@mapNotNull null
            val name = links.interfaceName ?: return@mapNotNull null
            val addresses = links.linkAddresses.filter {
                it.address is Inet4Address && !it.address.isLoopbackAddress && !it.address.isAnyLocalAddress
            }
            if (addresses.isEmpty()) return@mapNotNull null
            Triple(network, name, addresses)
        }.sortedBy { it.second }
        // XChat is a LAN app: pin both discovery and new HTTP/file-transfer sockets to
        // the same physical network, including when a VPN owns the system default route.
        val selected = candidates.firstOrNull()
        if (manager.boundNetworkForProcess != selected?.first) {
            check(manager.bindProcessToNetwork(selected?.first)) { "Cannot bind XChat to LAN network" }
            println("[LAN] Bound network: ${selected?.second ?: "none"}")
        }
        val result = JSONArray()
        selected?.let { (_, name, addresses) ->
            val index = runCatching { NetworkInterface.getByName(name)?.index }.getOrNull()
            addresses.forEach { address ->
                result.put(JSONObject().apply {
                    put("name", name)
                    put("system_name", name)
                    put("index", index ?: JSONObject.NULL)
                    put("ipv4", address.address.hostAddress)
                    put("prefix_length", address.prefixLength)
                    put("is_up", true)
                    put("is_loopback", false)
                })
            }
        }
        return result.toString()
    }

    override fun onStop() {
        releaseDiscoveryMulticastLock()
        super.onStop()
    }

    /**
     * 供 Rust JNI 调用：打开 SAF 文件选择器
     */
    @Keep
    fun launchSafFilePicker(mimeType: String) {
        runOnUiThread {
            safPickerLauncher.launch(arrayOf(if (mimeType == "image/*") mimeType else "*/*"))
        }
    }

    private fun stageSelectedAttachment(uri: Uri) {
        // The shared Rust transfer core expects a real file. Stream the user's
        // selected document off the UI thread, without loading an APK into JS.
        Thread({
            var directory: File? = null
            val result = try {
                var name = "attachment"
                contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
                    if (cursor.moveToFirst()) name = cursor.getString(0) ?: name
                }
                name = name.substringAfterLast('/').substringAfterLast('\\').ifBlank { "attachment" }
                if (name == "." || name == "..") name = "attachment"
                directory = File(filesDir, "attachments/${java.util.UUID.randomUUID()}").apply { mkdirs() }
                val pending = File(directory, ".pending")
                val target = File(directory, name)
                (contentResolver.openInputStream(uri) ?: error("无法读取所选文件")).use { input ->
                    pending.outputStream().use { output -> input.copyTo(output) }
                }
                check(pending.renameTo(target)) { "无法保存所选文件" }
                JSONObject().apply {
                    put("status", "ok"); put("path", target.absolutePath)
                    put("name", name); put("size", target.length())
                    put("mime_type", contentResolver.getType(uri) ?: "application/octet-stream")
                }
            } catch (error: Exception) {
                directory?.deleteRecursively()
                JSONObject().put("status", "error").put("message", "读取附件失败：${error.message}")
            }
            runOnUiThread { injectDataIntoWebView(result.toString(), 0, "__XCHAT_NATIVE_ATTACHMENT__", "xchat-native-attachment") }
        }, "xchat-attachment-import").start()
    }

    private fun handleSafSelectedFile(uri: Uri) {
        try {
            // 持久化读取权限
            val takeFlags = Intent.FLAG_GRANT_READ_URI_PERMISSION
            contentResolver.takePersistableUriPermission(uri, takeFlags)

            // 提取文件名和大小
            var fileName = "unknown_file"
            var fileSize: Long = 0
            contentResolver.query(uri, null, null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val nameIndex = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                    val sizeIndex = cursor.getColumnIndex(android.provider.OpenableColumns.SIZE)
                    if (nameIndex >= 0) fileName = cursor.getString(nameIndex)
                    if (sizeIndex >= 0) fileSize = cursor.getLong(sizeIndex)
                }
            }

            // 回调 Rust 侧，走 Tauri 事件总线广播给前端
            nativeOnSafFileSelected(uri.toString(), fileName, fileSize)

        } catch (e: Exception) {
            println("[MainActivity] SAF 文件选择持久化失败: ${e.message}")
            e.printStackTrace()
        }
    }

    // ═══════════════════════════════════════════════════════════
    // 拍照：调起系统相机，结果作为附件回到前端草稿
    // ═══════════════════════════════════════════════════════════

    /**
     * 供 Rust JNI 调用：拍照。没有权限就先申请，授权后自动继续。
     */
    @Keep
    fun launchCameraCapture() {
        runOnUiThread {
            val granted = ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
            if (granted) {
                launchCameraCaptureInternal()
            } else {
                cameraPermissionLauncher.launch(Manifest.permission.CAMERA)
            }
        }
    }

    /**
     * 照片落在应用私有目录（filesDir/media/native/camera），
     * 和 managed_image 的 outbox 一样是「发出去之前属于本应用」的临时文件；
     * 前端拿到的是真实文件路径（不是 content:// ），可以走已有的
     * send_conversation_file 链路。
     */
    private fun nativeCaptureDir(name: String): File {
        val dir = File(filesDir, "media/native/$name")
        if (!dir.exists()) dir.mkdirs()
        return dir
    }

    private fun launchCameraCaptureInternal() {
        val file = File(nativeCaptureDir("camera"), "xchat-photo-${System.currentTimeMillis()}.jpg")
        try {
            file.createNewFile()
            val uri = FileProvider.getUriForFile(
                this,
                "${applicationContext.packageName}.fileprovider",
                file,
            )
            pendingCameraFile = file
            cameraLauncher.launch(uri)
        } catch (e: ActivityNotFoundException) {
            // 设备上没有相机应用：给前端一个明确的状态，而不是静默失败
            pendingCameraFile = null
            file.delete()
            println("[MainActivity] 没有可用的相机应用: ${e.message}")
            pushCameraResult(JSONObject().put("status", "no_camera"))
        } catch (e: Exception) {
            pendingCameraFile = null
            file.delete()
            println("[MainActivity] 调起相机失败: ${e.message}")
            pushCameraResult(
                JSONObject().put("status", "failed").put("message", e.message ?: "")
            )
        }
    }

    private fun handleCameraResult(success: Boolean) {
        val file = pendingCameraFile
        pendingCameraFile = null
        if (success && file != null && file.exists() && file.length() > 0L) {
            println("[MainActivity] 拍照完成: ${file.absolutePath} (${file.length()} 字节)")
            pushCameraResult(JSONObject().apply {
                put("status", "ok")
                put("path", file.absolutePath)
                put("name", file.name)
                put("size", file.length())
                put("mime_type", "image/jpeg")
            })
            return
        }
        // 用户取消（或相机没写出内容）：删掉空文件，不留悬空附件
        val removed = file?.delete() ?: false
        println("[MainActivity] 拍照取消，清理临时文件: $removed")
        pushCameraResult(JSONObject().put("status", "cancelled"))
    }

    /** 复用既有的空投机制，把结果交给前端（见 injectDataIntoWebView）。 */
    private fun pushCameraResult(payload: JSONObject) {
        injectDataIntoWebView(
            payload.toString(),
            0,
            globalName = "__XCHAT_NATIVE_CAPTURE__",
            eventName = "xchat-native-capture",
        )
    }

    // ═══════════════════════════════════════════════════════════
    // 录音：MediaRecorder(MPEG_4 + AAC) → m4a 落到应用私有目录
    // ═══════════════════════════════════════════════════════════

    private fun voiceStatus(status: String, message: String? = null): String =
        JSONObject().apply {
            put("status", status)
            if (!message.isNullOrEmpty()) put("message", message)
        }.toString()

    /**
     * 供 Rust JNI 调用：开始录音，返回 JSON 状态字符串。
     * 返回 permission_denied 时已经顺带向用户发起了权限申请。
     */
    @Keep
    fun startVoiceRecording(): String {
        if (mediaRecorder != null) return voiceStatus("recording")

        if (ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            runOnUiThread { audioPermissionLauncher.launch(Manifest.permission.RECORD_AUDIO) }
            return voiceStatus("permission_denied")
        }

        val file = File(nativeCaptureDir("voice"), "xchat-voice-${System.currentTimeMillis()}.m4a")
        var recorder: MediaRecorder? = null
        return try {
            recorder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                MediaRecorder(this)
            } else {
                @Suppress("DEPRECATION")
                MediaRecorder()
            }
            recorder.setAudioSource(MediaRecorder.AudioSource.MIC)
            recorder.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
            recorder.setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
            recorder.setAudioEncodingBitRate(64_000)
            recorder.setAudioSamplingRate(44_100)
            recorder.setOutputFile(file.absolutePath)
            recorder.prepare()
            recorder.start()

            mediaRecorder = recorder
            recordingFile = file
            recordingStartedAt = System.currentTimeMillis()
            println("[MainActivity] 开始录音: ${file.absolutePath}")
            voiceStatus("recording")
        } catch (e: Exception) {
            println("[MainActivity] 录音启动失败: ${e.message}")
            runCatching { recorder?.release() }
            file.delete()
            voiceStatus("failed", e.message ?: "")
        }
    }

    /**
     * 供 Rust JNI 调用：结束录音，返回 JSON 状态字符串。
     * cancelled = true 表示前端判定为取消（上滑取消 / 时长过短），文件会被删除。
     */
    @Keep
    fun stopVoiceRecording(cancelled: Boolean): String {
        val recorder = mediaRecorder
        val file = recordingFile
        val durationMs = if (recordingStartedAt > 0L) {
            System.currentTimeMillis() - recordingStartedAt
        } else {
            0L
        }
        mediaRecorder = null
        recordingFile = null
        recordingStartedAt = 0L

        if (recorder == null) {
            file?.delete()
            return voiceStatus("cancelled")
        }

        var stopError: String? = null
        try {
            recorder.stop()
        } catch (e: Exception) {
            // 开始后立刻停止时 MediaRecorder 会抛 RuntimeException，属于正常情况
            stopError = e.message
        }
        runCatching { recorder.release() }

        if (cancelled || stopError != null || file == null || !file.exists() || file.length() <= 0L) {
            val removed = file?.delete() ?: false
            println("[MainActivity] 录音取消（$stopError），清理临时文件: $removed")
            return voiceStatus("cancelled")
        }

        // m4a 的 MIME 用 audio/mp4（MPEG_4 容器 + AAC）
        println("[MainActivity] 录音完成: ${file.absolutePath} (${durationMs}ms)")
        return JSONObject().apply {
            put("status", "ok")
            put("path", file.absolutePath)
            put("name", file.name)
            put("size", file.length())
            put("duration_ms", durationMs)
            put("mime_type", "audio/mp4")
        }.toString()
    }

    private fun checkNotificationLaunch(intent: Intent?) {
        if (intent == null) return
        val action = intent.getStringExtra(ACTION_INTENT_KEY)
        if (action == "tap") {
            println("[MainActivity] 冷启动来自通知点击")
            // 延迟等 WebView 就绪后再通知 JS
            window.decorView.postDelayed({
                notifyNotificationClicked()
            }, 1000)
        }
    }

    private fun registerShareReceiver() {
        shareReceiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context?, intent: Intent?) {
                println("[MainActivity] 收到分享广播")
                checkAndPushSharedFiles()
            }
        }
        val filter = IntentFilter("com.xchat.app.SHARE_RECEIVED")
        registerReceiver(shareReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
    }

    // 核心推送函数
    private fun checkAndPushSharedFiles() {
        val files = ShareDataHolder.sharedFiles
        if (files == null || files.isEmpty()) return

        println("[MainActivity] 准备推送 ${files.size} 个文件到前端")
        
        // 一旦取出数据，立刻清空保险箱！
        // 这样哪怕 onResume 和 广播 同时触发，第二个进来的也只能拿到 null，彻底杜绝双重注入！
        ShareDataHolder.sharedFiles = null

        val jsonArray = JSONArray()
        files.forEach { file ->
            val jsonObj = JSONObject().apply {
                put("uri", file.uri)
                put("fileName", file.fileName)
                put("fileSize", file.fileSize)
                put("mimeType", file.mimeType)
                put("fd", file.fd)
            }
            jsonArray.put(jsonObj)
        }
        val jsonString = jsonArray.toString()
        
        injectDataIntoWebView(jsonString, 0)
    }

    // 智能重试空投机制
    // globalName / eventName 让同一个机制可以服务多条链路（分享进来的文件、相机结果…），
    // 默认值保持分享链路的原有行为不变。
    private fun injectDataIntoWebView(
        jsonString: String,
        attempt: Int,
        globalName: String = "__ANDROID_SHARED_FILES__",
        eventName: String = "android-share-received",
    ) {
        val maxAttempts = 20 // 允许重试20次（10秒），彻底防住冷启动慢的问题
        if (attempt >= maxAttempts) {
            println("[MainActivity] 放弃注入分享数据，重试次数过多")
            return
        }

        if (webView == null) {
            webView = findWebView(window.decorView)
        }

        if (webView != null) {
            runOnUiThread {
                webView?.evaluateJavascript(
                    """
                    (function() {
                        // 确保 JS 运行环境已存在
                        if (typeof window !== 'undefined') {
                            // 直接把数据空投进 window 全局变量
                            window.$globalName = $jsonString;
                            console.log('[MainActivity->JS] 数据已成功空投到 window.$globalName');
                            // 触发事件通知前端
                            if (window.dispatchEvent) {
                                window.dispatchEvent(new CustomEvent('$eventName', { detail: window.$globalName }));
                            }
                            return "success";
                        }
                        return "not_ready";
                    })();
                    """.trimIndent()
                ) { result ->
                    if (result == "\"success\"") {
                        println("[MainActivity] 数据成功推送到前端 (尝试 ${attempt + 1})")
                        // 确保只推送一次，推送成功后立刻清空原生层保险箱
                        ShareDataHolder.sharedFiles = null 
                    } else {
                        println("[MainActivity] 前端 window 未就绪，500ms 后重试...")
                        window.decorView.postDelayed({ injectDataIntoWebView(jsonString, attempt + 1, globalName, eventName) }, 500)
                    }
                }
            }
        } else {
            println("[MainActivity] 找不到 WebView，500ms 后重试...")
            window.decorView.postDelayed({ injectDataIntoWebView(jsonString, attempt + 1, globalName, eventName) }, 500)
        }
    }

    override fun onDestroy() {
        // 录音中途被销毁（旋转 / 退出）时释放麦克风，避免占用音频资源
        runCatching {
            mediaRecorder?.let { recorder ->
                runCatching { recorder.stop() }
                runCatching { recorder.release() }
            }
        }
        mediaRecorder = null
        recordingFile = null
        releaseDiscoveryMulticastLock()
        super.onDestroy()
        // 注销广播接收器
        shareReceiver?.let {
            unregisterReceiver(it)
            println("[MainActivity] 广播接收器已注销")
        }
    }

    private fun acquireDiscoveryMulticastLock() {
        if (discoveryMulticastLock?.isHeld == true) return
        runCatching {
            val wifiManager = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
            discoveryMulticastLock = wifiManager.createMulticastLock(
                "$packageName:xchat-discovery"
            ).apply {
                setReferenceCounted(false)
                acquire()
            }
            println("[MainActivity] 局域网发现 multicast lock 已获取")
        }.onFailure { error ->
            println("[MainActivity] 获取 multicast lock 失败: ${error.message}")
            discoveryMulticastLock = null
        }
    }

    private fun releaseDiscoveryMulticastLock() {
        runCatching {
            discoveryMulticastLock?.takeIf { it.isHeld }?.release()
        }.onFailure { error ->
            println("[MainActivity] 释放 multicast lock 失败: ${error.message}")
        }
        discoveryMulticastLock = null
    }

    private fun findWebView(view: android.view.View): WebView? {
        println("[MainActivity] 检查 View: ${view.javaClass.name}")
        
        if (view is WebView) {
            println("[MainActivity] 找到 WebView: ${view.javaClass.name}")
            return view
        }
        if (view is android.view.ViewGroup) {
            for (i in 0 until view.childCount) {
                val child = view.getChildAt(i)
                val result = findWebView(child)
                if (result != null) {
                    return result
                }
            }
        }
        return null
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        
        // 检测通知点击：NotificationUserAction == "tap" 表示通知被点击
        val action = intent.getStringExtra(ACTION_INTENT_KEY)
        if (action == "tap") {
            println("[MainActivity] 通知被点击")
            // 通知 JS 从 localStorage 读取 from_id 并导航
            notifyNotificationClicked()
        }
    }
    
    override fun onResume() {
        super.onResume()
        println("[MainActivity] onResume 被调用")
        checkAndPushSharedFiles()
    }
    

    private fun notifyNotificationClicked() {
        // 通知前端从 localStorage 读取 pendingFromId 并导航
        notifyWebViewWithRetry(0, """
            (function() {
                console.log('[MainActivity] 通知被点击，尝试导航');
                var fromId = localStorage.getItem('pendingNotificationFromId');
                if (fromId) {
                    localStorage.removeItem('pendingNotificationFromId');
                    window.dispatchEvent(new CustomEvent('notification-tapped', {detail: {fromId: fromId}}));
                }
            })();
        """.trimIndent())
    }
    
    private fun notifyWebView() {
        println("[MainActivity] 准备通知 WebView")
        
        // 使用递归重试机制
        notifyWebViewWithRetry(0)
    }
    
    private fun notifyWebViewWithRetry(attempt: Int, jsCode: String = """
        (function() {
            console.log('[MainActivity] 触发 android-share-received 事件');
            window.dispatchEvent(new CustomEvent('android-share-received'));
        })();
    """.trimIndent()) {
        val maxAttempts = 10
        val delayMs = 500L
        
        if (attempt >= maxAttempts) {
            println("[MainActivity] 达到最大重试次数，放弃通知")
            return
        }
        
        window.decorView.postDelayed({
            // 尝试重新查找 WebView
            if (webView == null) {
                webView = findWebView(window.decorView)
            }
            
            if (webView != null) {
                println("[MainActivity] WebView 已就绪（尝试 ${attempt + 1}），发送事件")
                runOnUiThread {
                    try {
                        webView?.evaluateJavascript(jsCode,
                            { result ->
                                println("[MainActivity] JavaScript 执行结果: $result")
                            }
                        )
                        println("[MainActivity] 已触发 android-share-received 事件")
                    } catch (e: Exception) {
                        println("[MainActivity] 执行 JavaScript 失败: ${e.message}")
                    }
                }
            } else {
                println("[MainActivity] WebView 未就绪（尝试 ${attempt + 1}），继续重试...")
                notifyWebViewWithRetry(attempt + 1, jsCode)
            }
        }, delayMs)
    }

    // Return errors to Rust; never substitute opening the file for showing its folder.
    @Keep
    fun revealFileDirectory(filePath: String): String {
        return try {
            val directoryUri: Uri
            if (filePath.startsWith("content://")) {
                val source = Uri.parse(filePath)
                val documentId = DocumentsContract.getDocumentId(source)
                val parentId = if (source.authority == "com.android.externalstorage.documents") {
                    val volume = documentId.substringBefore(':')
                    val relative = documentId.substringAfter(':', "")
                    "$volume:${relative.substringBeforeLast('/', "")}"
                } else if (Build.VERSION.SDK_INT >= 26) {
                    val path = DocumentsContract.findDocumentPath(contentResolver, source)?.path
                    path?.dropLast(1)?.lastOrNull()
                } else null
                if (parentId == null) return "该文件来源未提供所在目录，请在系统文件管理器中查找"
                directoryUri = DocumentsContract.buildDocumentUri(source.authority, parentId)
            } else {
                val file = File(filePath).canonicalFile
                if (!file.isFile) return "文件已不存在，请重新接收"
                val parent = file.parentFile ?: return "无法找到文件目录"
                val managed = mapOf("downloads" to File(filesDir, "Downloads"), "media" to File(filesDir, "media/native"), "attachments" to File(filesDir, "attachments"))
                val root = managed.entries.firstOrNull { (_, base) ->
                    parent == base.canonicalFile || parent.path.startsWith(base.canonicalPath + File.separator)
                }
                directoryUri = if (root != null) {
                    val relative = parent.relativeTo(root.value.canonicalFile).path.replace(File.separatorChar, '/')
                    DocumentsContract.buildDocumentUri("$packageName.documents", "${root.key}:$relative")
                } else {
                    val external = android.os.Environment.getExternalStorageDirectory().canonicalFile
                    if (parent != external && !parent.path.startsWith(external.path + File.separator)) {
                        return "该目录无法由系统文件管理器访问"
                    }
                    DocumentsContract.buildDocumentUri("com.android.externalstorage.documents", "primary:${parent.relativeTo(external).path}")
                }
            }
            val intent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(directoryUri, DocumentsContract.Document.MIME_TYPE_DIR)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            // Honor a chosen default. With no default, use the system Files
            // handler, not unrelated apps which claim every MIME type.
            val handlers = packageManager.queryIntentActivities(intent, PackageManager.MATCH_DEFAULT_ONLY)
            val resolved = packageManager.resolveActivity(intent, PackageManager.MATCH_DEFAULT_ONLY)?.activityInfo
            val chosen = handlers.firstOrNull { it.activityInfo.packageName == resolved?.packageName && it.activityInfo.name == resolved?.name }
                ?: handlers.firstOrNull {
                    packageManager.checkPermission("android.permission.MANAGE_DOCUMENTS", it.activityInfo.packageName) == PackageManager.PERMISSION_GRANTED
                }
            chosen?.activityInfo?.let { intent.setClassName(it.packageName, it.name) }
            // The UI thread executes the launch; report the launch result synchronously.
            val result = java.util.concurrent.FutureTask<String> {
                try { startActivity(intent); "" }
                catch (error: Exception) { "打开文件目录失败：${error.message}" }
            }
            runOnUiThread(result)
            result.get(5, java.util.concurrent.TimeUnit.SECONDS)
        } catch (error: Exception) {
            "打开文件目录失败：${error.message}"
        }
    }

    // 打开文件（用对应的应用打开）
    @Keep
    fun openFile(filePath: String) {
        try {
            println("[MainActivity] 准备打开文件: $filePath")

            val uri: Uri
            val mimeType: String

            if (filePath.startsWith("content://")) {
                uri = Uri.parse(filePath)
                mimeType = contentResolver.getType(uri) ?: "*/*"
                println("[MainActivity] 使用 content URI: $uri")
            } else {
                val file = File(filePath)
                if (!file.exists()) {
                    println("[MainActivity] 文件不存在: $filePath")
                    return
                }
                uri = FileProvider.getUriForFile(
                    this,
                    "${applicationContext.packageName}.fileprovider",
                    file
                )
                mimeType = contentResolver.getType(uri) ?: "*/*"
                println("[MainActivity] FileProvider URI: $uri")
            }

            println("[MainActivity] MIME 类型: $mimeType")

            val intent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, mimeType)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }

            try {
                startActivity(intent)
                println("[MainActivity] 打开文件 Intent 已启动")
            } catch (e: SecurityException) {
                // content URI 权限过期，降级为 */* 再试一次
                println("[MainActivity] 权限异常，降级为 */* 重试: ${e.message}")
                val fallbackIntent = Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(uri, "*/*")
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                startActivity(fallbackIntent)
                println("[MainActivity] 降级打开文件 Intent 已启动")
            }
        } catch (e: Exception) {
            println("[MainActivity] 打开文件失败: ${e.message}")
            e.printStackTrace()
        }
    }

    // 分享文件到其他应用
    @Keep   // <--- 就是这块免死金牌！告诉混淆器绝对不要动这个函数
    fun shareFile(filePath: String) {
        try {
            println("[MainActivity] 准备分享文件: $filePath")
            
            val uri: Uri
            val mimeType: String
            
            if (filePath.startsWith("content://")) {
                // 已经是 content URI，直接使用
                uri = Uri.parse(filePath)
                mimeType = contentResolver.getType(uri) ?: "*/*"
                println("[MainActivity] 使用 content URI: $uri")
            } else {
                // 普通文件路径，使用 FileProvider
                val file = File(filePath)
                if (!file.exists()) {
                    println("[MainActivity] 文件不存在: $filePath")
                    return
                }
                
                uri = FileProvider.getUriForFile(
                    this,
                    "${applicationContext.packageName}.fileprovider",
                    file
                )
                mimeType = contentResolver.getType(uri) ?: "*/*"
                println("[MainActivity] FileProvider URI: $uri")
            }
            
            println("[MainActivity] MIME 类型: $mimeType")
            
            val intent = Intent(Intent.ACTION_SEND).apply {
                type = mimeType
                putExtra(Intent.EXTRA_STREAM, uri)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            }

            // 附加 ClipData，让系统 UI（分享面板缩略图）也能合法访问 URI，消除 SecurityException 日志
            val clipData = android.content.ClipData.newUri(contentResolver, "share_file", uri)
            intent.clipData = clipData
            
            // 创建分享选择器并授予权限
            val chooser = Intent.createChooser(intent, "分享文件").apply {
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            }
            
            // 显示分享选择器
            startActivity(chooser)
            println("[MainActivity] 分享选择器已启动")
        } catch (e: Exception) {
            println("[MainActivity] 分享文件失败: ${e.message}")
            e.printStackTrace()
        }
    }
}
