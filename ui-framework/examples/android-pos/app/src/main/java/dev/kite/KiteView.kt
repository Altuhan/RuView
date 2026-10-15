package dev.kite

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.text.InputType
import android.util.AttributeSet
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import java.nio.ByteBuffer

/**
 * An Android View that shows a Kite component.
 *
 * Rendering happens in Rust (the native library must be loaded first, e.g.
 * `System.loadLibrary("kitepos")`): each frame is drawn into a direct
 * ByteBuffer and copied into a Bitmap. Touches and keyboard input are
 * forwarded to the component; its public callbacks arrive in [callbackListener].
 */
class KiteView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : View(context, attrs) {

    fun interface CallbackListener {
        fun onCallback(name: String, args: Array<String>)
    }

    var callbackListener: CallbackListener? = null

    private var handle: Long = nativeCreate(this)
    private var bitmap: Bitmap? = null
    private var buffer: ByteBuffer? = null
    private var dirty = true

    init {
        isFocusable = true
        isFocusableInTouchMode = true
    }

    /** A public property of the component as text. */
    fun property(name: String): String = nativeGetProperty(handle, name)

    /** Sets a public property from text; returns "" or an error message. */
    fun setProperty(name: String, value: String): String =
        nativeSetProperty(handle, name, value).also { invalidate() }

    /** `[x, y, width, height]` of the element with [id], in view pixels. */
    fun elementRect(id: String): FloatArray = nativeElementRect(handle, id)

    /** Frees the native component. The view must not be used afterwards. */
    fun release() {
        if (handle != 0L) {
            nativeDestroy(handle)
            handle = 0L
        }
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        if (w <= 0 || h <= 0 || handle == 0L) return
        nativeResize(handle, w, h, resources.displayMetrics.density)
        bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        buffer = ByteBuffer.allocateDirect(w * h * 4)
        dirty = true
    }

    override fun onDraw(canvas: Canvas) {
        val bmp = bitmap ?: return
        val buf = buffer ?: return
        if (handle == 0L) return
        if (dirty || nativeNeedsRedraw(handle)) {
            nativeRender(handle, buf, bmp.width, bmp.height)
            buf.rewind()
            bmp.copyPixelsFromBuffer(buf)
            buf.rewind()
            dirty = false
        }
        canvas.drawBitmap(bmp, 0f, 0f, null)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (handle == 0L) return false
        val action = when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> 0
            MotionEvent.ACTION_UP -> 1
            MotionEvent.ACTION_MOVE -> 2
            MotionEvent.ACTION_CANCEL -> 3
            else -> return true
        }
        if (action == 0) requestFocus()
        if (nativeTouch(handle, action, event.x, event.y)) invalidate()
        if (action == 1) {
            updateKeyboard()
            performClick()
        }
        return true
    }

    override fun performClick(): Boolean {
        super.performClick()
        return true
    }

    private fun updateKeyboard() {
        val imm = context.getSystemService(InputMethodManager::class.java) ?: return
        if (nativeWantsKeyboard(handle)) {
            imm.restartInput(this)
            imm.showSoftInput(this, 0)
        } else {
            imm.hideSoftInputFromWindow(windowToken, 0)
        }
    }

    private fun hideKeyboard() {
        context.getSystemService(InputMethodManager::class.java)?.hideSoftInputFromWindow(windowToken, 0)
    }

    override fun onCheckIsTextEditor(): Boolean = handle != 0L && nativeWantsKeyboard(handle)

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        outAttrs.imeOptions = EditorInfo.IME_ACTION_DONE or EditorInfo.IME_FLAG_NO_EXTRACT_UI
        return object : BaseInputConnection(this, false) {
            override fun commitText(text: CharSequence, newCursorPosition: Int): Boolean {
                if (nativeText(handle, text.toString())) invalidate()
                return true
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                repeat(beforeLength) { nativeKey(handle, KeyEvent.KEYCODE_DEL, false) }
                invalidate()
                return true
            }

            override fun performEditorAction(actionCode: Int): Boolean {
                nativeKey(handle, KeyEvent.KEYCODE_ENTER, false)
                hideKeyboard()
                invalidate()
                return true
            }

            override fun sendKeyEvent(event: KeyEvent): Boolean {
                if (event.action == KeyEvent.ACTION_DOWN) handleKey(event)
                return true
            }
        }
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean =
        handleKey(event) || super.onKeyDown(keyCode, event)

    private fun handleKey(event: KeyEvent): Boolean {
        if (handle == 0L) return false
        var handled = nativeKey(handle, event.keyCode, event.isShiftPressed)
        if (!handled) {
            val ch = event.unicodeChar
            if (ch != 0 && !Character.isISOControl(ch)) {
                nativeText(handle, String(Character.toChars(ch)))
                handled = true
            }
        }
        if (handled) {
            // "Enter" in a text field works like the keyboard's Done action.
            if (event.keyCode == KeyEvent.KEYCODE_ENTER) hideKeyboard()
            invalidate()
        }
        return handled
    }

    /** Called from native code when the component invokes a public callback. */
    @Suppress("unused")
    private fun onKiteCallback(name: String, args: Array<String>) {
        callbackListener?.onCallback(name, args)
        invalidate()
    }

    private external fun nativeCreate(view: KiteView): Long
    private external fun nativeDestroy(handle: Long)
    private external fun nativeResize(handle: Long, width: Int, height: Int, density: Float)
    private external fun nativeTouch(handle: Long, action: Int, x: Float, y: Float): Boolean
    private external fun nativeText(handle: Long, text: String): Boolean
    private external fun nativeKey(handle: Long, code: Int, shift: Boolean): Boolean
    private external fun nativeWantsKeyboard(handle: Long): Boolean
    private external fun nativeNeedsRedraw(handle: Long): Boolean
    private external fun nativeRender(handle: Long, buffer: ByteBuffer, width: Int, height: Int)
    private external fun nativeGetProperty(handle: Long, name: String): String
    private external fun nativeSetProperty(handle: Long, name: String, value: String): String
    private external fun nativeElementRect(handle: Long, id: String): FloatArray
}
