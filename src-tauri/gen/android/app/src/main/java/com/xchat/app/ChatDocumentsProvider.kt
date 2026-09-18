package com.xchat.app

import android.database.Cursor
import android.database.MatrixCursor
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import android.provider.DocumentsProvider
import android.webkit.MimeTypeMap
import java.io.File
import java.io.FileNotFoundException

/** System Files can browse attachments without exposing the database or app settings. */
class ChatDocumentsProvider : DocumentsProvider() {
    private val roots get() = mapOf(
        "downloads" to File(requireNotNull(context).filesDir, "Downloads"),
        "media" to File(requireNotNull(context).filesDir, "media/native"),
        "attachments" to File(requireNotNull(context).filesDir, "attachments"),
    )

    override fun onCreate() = true

    private fun file(documentId: String): File {
        val root = roots[documentId.substringBefore(':')]?.canonicalFile
            ?: throw FileNotFoundException("Unknown attachment root")
        val relative = documentId.substringAfter(':', "")
        val result = File(root, relative).canonicalFile
        if (result != root && !result.path.startsWith(root.path + File.separator)) {
            throw FileNotFoundException("Outside attachment directory")
        }
        if (!result.exists()) throw FileNotFoundException("Attachment no longer exists")
        return result
    }

    private fun id(file: File): String {
        val canonical = file.canonicalFile
        val entry = roots.entries.firstOrNull { (_, value) ->
            canonical == value.canonicalFile || canonical.path.startsWith(value.canonicalPath + File.separator)
        } ?: throw FileNotFoundException("Outside attachment directory")
        return entry.key + ":" + canonical.relativeTo(entry.value.canonicalFile).path.replace(File.separatorChar, '/')
    }

    override fun queryRoots(projection: Array<out String>?): Cursor {
        val cursor = MatrixCursor(projection ?: arrayOf(
            DocumentsContract.Root.COLUMN_ROOT_ID, DocumentsContract.Root.COLUMN_DOCUMENT_ID,
            DocumentsContract.Root.COLUMN_TITLE, DocumentsContract.Root.COLUMN_FLAGS,
            DocumentsContract.Root.COLUMN_ICON, DocumentsContract.Root.COLUMN_MIME_TYPES,
        ))
        roots.forEach { (key, directory) ->
            directory.mkdirs()
            cursor.newRow().apply {
                add(DocumentsContract.Root.COLUMN_ROOT_ID, key)
                add(DocumentsContract.Root.COLUMN_DOCUMENT_ID, "$key:")
                add(DocumentsContract.Root.COLUMN_TITLE, when (key) { "downloads" -> "XChat · 接收的文件"; "media" -> "XChat · 拍摄与录音"; else -> "XChat · 发送的文件" })
                add(DocumentsContract.Root.COLUMN_FLAGS, DocumentsContract.Root.FLAG_LOCAL_ONLY or DocumentsContract.Root.FLAG_SUPPORTS_IS_CHILD)
                add(DocumentsContract.Root.COLUMN_ICON, com.xchat.app.R.mipmap.ic_launcher)
                add(DocumentsContract.Root.COLUMN_MIME_TYPES, "*/*")
            }
        }
        return cursor
    }

    private fun cursor(projection: Array<out String>?) = MatrixCursor(projection ?: arrayOf(
        DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME,
        DocumentsContract.Document.COLUMN_MIME_TYPE, DocumentsContract.Document.COLUMN_SIZE,
        DocumentsContract.Document.COLUMN_LAST_MODIFIED, DocumentsContract.Document.COLUMN_FLAGS,
    ))

    private fun include(cursor: MatrixCursor, file: File) {
        // A partially received attachment is never offered to other apps.
        if (file.name.startsWith(".") || file.name.endsWith(".part")) return
        val mime = if (file.isDirectory) DocumentsContract.Document.MIME_TYPE_DIR else
            MimeTypeMap.getSingleton().getMimeTypeFromExtension(file.extension.lowercase()) ?: "application/octet-stream"
        cursor.newRow().apply {
            add(DocumentsContract.Document.COLUMN_DOCUMENT_ID, id(file))
            add(DocumentsContract.Document.COLUMN_DISPLAY_NAME, file.name)
            add(DocumentsContract.Document.COLUMN_MIME_TYPE, mime)
            add(DocumentsContract.Document.COLUMN_SIZE, if (file.isFile) file.length() else null)
            add(DocumentsContract.Document.COLUMN_LAST_MODIFIED, file.lastModified())
            add(DocumentsContract.Document.COLUMN_FLAGS, 0)
        }
    }

    override fun queryDocument(documentId: String, projection: Array<out String>?) =
        cursor(projection).also { include(it, file(documentId)) }

    override fun queryChildDocuments(parentDocumentId: String, projection: Array<out String>?, sortOrder: String?) =
        cursor(projection).also { cursor ->
            file(parentDocumentId).listFiles()?.sortedBy { it.name.lowercase() }?.forEach { child ->
                // Canonical validation also excludes symlinks escaping the managed directory.
                runCatching { include(cursor, file(id(child))) }
            }
        }

    override fun isChildDocument(parentDocumentId: String, documentId: String): Boolean =
        file(documentId).path.startsWith(file(parentDocumentId).path + File.separator)

    override fun findDocumentPath(parentDocumentId: String?, childDocumentId: String): DocumentsContract.Path {
        val rootId = childDocumentId.substringBefore(':')
        val root = file(parentDocumentId ?: "$rootId:")
        var current = file(childDocumentId)
        if (current != root && !current.path.startsWith(root.path + File.separator)) {
            throw FileNotFoundException("Document is outside the requested tree")
        }
        val ancestors = mutableListOf<String>()
        while (true) {
            ancestors.add(0, id(current))
            if (current == root) break
            current = current.parentFile ?: throw FileNotFoundException("Missing parent")
        }
        return DocumentsContract.Path(if (parentDocumentId == null) rootId else null, ancestors)
    }

    override fun openDocument(documentId: String, mode: String, signal: CancellationSignal?): ParcelFileDescriptor {
        if (mode != "r") throw FileNotFoundException("Attachments are read-only")
        signal?.throwIfCanceled()
        return ParcelFileDescriptor.open(file(documentId), ParcelFileDescriptor.MODE_READ_ONLY)
    }
}
