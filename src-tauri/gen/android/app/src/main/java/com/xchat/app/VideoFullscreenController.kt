package com.xchat.app

import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.drawable.Drawable
import android.net.Uri
import android.os.Message
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.ConsoleMessage
import android.webkit.GeolocationPermissions
import android.webkit.JsPromptResult
import android.webkit.JsResult
import android.webkit.PermissionRequest
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebStorage
import android.webkit.WebView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature

/** Adds fullscreen to Wry without editing its generated client or losing its callbacks. */
internal class VideoFullscreenController private constructor(
    private val activity: MainActivity,
    private val webView: WebView,
    private val delegate: WebChromeClient,
) {
    private val content = activity.findViewById<ViewGroup>(android.R.id.content)
    private var customView: View? = null
    private var customCallback: WebChromeClient.CustomViewCallback? = null
    private var originalBackground: Drawable? = null
    private var originalFullscreen = false
    private var originalStatusVisible = true
    private var originalNavigationVisible = true
    private var originalBarsBehavior = 0
    private var originalLightStatus = false
    private var originalLightNavigation = false

    val isShowing: Boolean get() = customView != null

    private fun show(view: View, callback: WebChromeClient.CustomViewCallback) {
        if (isShowing || activity.isFinishing || activity.isDestroyed || view.parent != null) {
            callback.onCustomViewHidden()
            return
        }
        val window = activity.window
        val bars = WindowCompat.getInsetsController(window, window.decorView)
        val insets = ViewCompat.getRootWindowInsets(content)
        originalBackground = content.background
        originalFullscreen = window.attributes.flags and WindowManager.LayoutParams.FLAG_FULLSCREEN != 0
        originalStatusVisible = insets?.isVisible(WindowInsetsCompat.Type.statusBars()) ?: true
        originalNavigationVisible = insets?.isVisible(WindowInsetsCompat.Type.navigationBars()) ?: true
        originalBarsBehavior = bars.systemBarsBehavior
        originalLightStatus = bars.isAppearanceLightStatusBars
        originalLightNavigation = bars.isAppearanceLightNavigationBars
        customView = view
        customCallback = callback
        content.setBackgroundColor(Color.BLACK)
        // Keep the original WebView visible and mounted. Hiding it can report a
        // background transition and pause the very video being shown fullscreen.
        content.addView(view, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        window.addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)
        bars.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        bars.isAppearanceLightStatusBars = false
        bars.isAppearanceLightNavigationBars = false
        bars.hide(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.ime())
        ViewCompat.requestApplyInsets(content)
    }

    private fun hide(notifyPage: Boolean) {
        val view = customView ?: return
        val callback = customCallback
        customView = null
        customCallback = null
        (view.parent as? ViewGroup)?.removeView(view)
        content.background = originalBackground
        originalBackground = null
        val window = activity.window
        if (!originalFullscreen) window.clearFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)
        val bars = WindowCompat.getInsetsController(window, window.decorView)
        bars.systemBarsBehavior = originalBarsBehavior
        bars.isAppearanceLightStatusBars = originalLightStatus
        bars.isAppearanceLightNavigationBars = originalLightNavigation
        if (originalStatusVisible) bars.show(WindowInsetsCompat.Type.statusBars())
        else bars.hide(WindowInsetsCompat.Type.statusBars())
        if (originalNavigationVisible) bars.show(WindowInsetsCompat.Type.navigationBars())
        else bars.hide(WindowInsetsCompat.Type.navigationBars())
        ViewCompat.requestApplyInsets(content)
        if (notifyPage) callback?.onCustomViewHidden()
    }

    fun exit(): Boolean {
        if (!isShowing) return false
        hide(true)
        return true
    }

    fun pauseForBackground() {
        if (isShowing) {
            webView.evaluateJavascript("document.querySelectorAll('video,audio').forEach(function(player){player.pause();})", null)
        }
    }

    fun dispose() {
        hide(true)
        webView.webChromeClient = delegate
    }

    @Suppress("DEPRECATION")
    private val client = object : WebChromeClient() {
        override fun onShowCustomView(view: View, callback: CustomViewCallback) = show(view, callback)
        override fun onShowCustomView(view: View, requestedOrientation: Int, callback: CustomViewCallback) = show(view, callback)
        override fun onHideCustomView() = hide(false)
        override fun onProgressChanged(view: WebView, progress: Int) = delegate.onProgressChanged(view, progress)
        override fun onReceivedTitle(view: WebView, title: String) = delegate.onReceivedTitle(view, title)
        override fun onReceivedIcon(view: WebView, icon: Bitmap) = delegate.onReceivedIcon(view, icon)
        override fun onReceivedTouchIconUrl(view: WebView, url: String, precomposed: Boolean) = delegate.onReceivedTouchIconUrl(view, url, precomposed)
        override fun onRequestFocus(view: WebView) = delegate.onRequestFocus(view)
        override fun onCloseWindow(window: WebView) = delegate.onCloseWindow(window)
        override fun onCreateWindow(view: WebView, isDialog: Boolean, isUserGesture: Boolean, resultMsg: Message): Boolean = delegate.onCreateWindow(view, isDialog, isUserGesture, resultMsg)
        override fun onJsAlert(view: WebView, url: String, message: String, result: JsResult): Boolean = delegate.onJsAlert(view, url, message, result)
        override fun onJsConfirm(view: WebView, url: String, message: String, result: JsResult): Boolean = delegate.onJsConfirm(view, url, message, result)
        override fun onJsPrompt(view: WebView, url: String, message: String, defaultValue: String, result: JsPromptResult): Boolean = delegate.onJsPrompt(view, url, message, defaultValue, result)
        override fun onJsBeforeUnload(view: WebView, url: String, message: String, result: JsResult): Boolean = delegate.onJsBeforeUnload(view, url, message, result)
        override fun onJsTimeout(): Boolean = delegate.onJsTimeout()
        override fun onConsoleMessage(message: ConsoleMessage): Boolean = delegate.onConsoleMessage(message)
        override fun onConsoleMessage(message: String, lineNumber: Int, sourceID: String) = delegate.onConsoleMessage(message, lineNumber, sourceID)
        override fun onGeolocationPermissionsShowPrompt(origin: String, callback: GeolocationPermissions.Callback) = delegate.onGeolocationPermissionsShowPrompt(origin, callback)
        override fun onGeolocationPermissionsHidePrompt() = delegate.onGeolocationPermissionsHidePrompt()
        override fun onPermissionRequest(request: PermissionRequest) = delegate.onPermissionRequest(request)
        override fun onPermissionRequestCanceled(request: PermissionRequest) = delegate.onPermissionRequestCanceled(request)
        override fun onShowFileChooser(webView: WebView, filePathCallback: ValueCallback<Array<Uri?>?>, fileChooserParams: FileChooserParams): Boolean = delegate.onShowFileChooser(webView, filePathCallback, fileChooserParams)
        override fun getDefaultVideoPoster(): Bitmap? = delegate.defaultVideoPoster
        override fun getVideoLoadingProgressView(): View? = delegate.videoLoadingProgressView
        override fun getVisitedHistory(callback: ValueCallback<Array<String>>) = delegate.getVisitedHistory(callback)
        override fun onExceededDatabaseQuota(url: String, databaseIdentifier: String, quota: Long, estimatedDatabaseSize: Long, totalQuota: Long, quotaUpdater: WebStorage.QuotaUpdater) = delegate.onExceededDatabaseQuota(url, databaseIdentifier, quota, estimatedDatabaseSize, totalQuota, quotaUpdater)
    }

    companion object {
        fun install(activity: MainActivity, webView: WebView): VideoFullscreenController? {
            if (!WebViewFeature.isFeatureSupported(WebViewFeature.GET_WEB_CHROME_CLIENT)) {
                println("[VideoFullscreen] WebView does not support reading its Chrome client")
                return null
            }
            val delegate = WebViewCompat.getWebChromeClient(webView) ?: return null
            return VideoFullscreenController(activity, webView, delegate).also {
                webView.webChromeClient = it.client
            }
        }
    }
}
