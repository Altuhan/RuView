package dev.kite.pos

import android.os.Build
import android.os.SystemClock
import android.util.Log
import android.view.KeyEvent
import android.view.MotionEvent
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Rings up a sale on a device or emulator through real touch and key events. */
@RunWith(AndroidJUnit4::class)
class PosFlowTest {

    private val inst = InstrumentationRegistry.getInstrumentation()

    private fun screenshot(name: String) {
        // Runs as the shell user, so the file can be pulled with adb.
        inst.uiAutomation.executeShellCommand("screencap -p /data/local/tmp/kite-$name.png").close()
        SystemClock.sleep(500)
    }

    @Test
    fun ringsUpACashSale() {
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var activity: MainActivity
            scenario.onActivity { activity = it }
            inst.waitForIdleSync()
            val view = activity.kite

            fun prop(name: String): String {
                var v = ""
                inst.runOnMainSync { v = view.property(name) }
                return v
            }

            fun tap(id: String) {
                var r = FloatArray(4)
                val loc = IntArray(2)
                inst.runOnMainSync {
                    r = view.elementRect(id)
                    view.getLocationOnScreen(loc)
                }
                assertTrue("element `$id` has no size", r[2] > 0f && r[3] > 0f)
                val x = loc[0] + r[0] + r[2] / 2
                val y = loc[1] + r[1] + r[3] / 2
                val t = SystemClock.uptimeMillis()
                inst.sendPointerSync(MotionEvent.obtain(t, t, MotionEvent.ACTION_DOWN, x, y, 0))
                inst.sendPointerSync(MotionEvent.obtain(t, t + 60, MotionEvent.ACTION_UP, x, y, 0))
                inst.waitForIdleSync()
            }

            screenshot("1-start")
            tap("sourdough")
            tap("sourdough")
            tap("coffee")
            tap("plu")
            inst.sendStringSync("4011")
            inst.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
            inst.waitForIdleSync()
            SystemClock.sleep(800) // let the soft keyboard close
            assertEquals("4", prop("items"))
            // (2 x 5.50 + 11.99 + 0.69) x 1.08 tax = 25.5744
            assertEquals("25.57", prop("total"))
            screenshot("2-cart")

            tap("charge")
            assertEquals("pay", prop("screen"))
            tap("cash-chip")
            tap("tender-fifty")
            assertEquals("24.43", prop("change"))
            screenshot("3-cash")

            tap("complete")
            assertEquals("done", prop("screen"))
            assertEquals(listOf("25.57", "cash"), activity.sales.last())
            screenshot("4-done")

            tap("new-sale")
            assertEquals("0", prop("items"))
            Log.i(
                MainActivity.TAG,
                "KITE_EVIDENCE sale flow passed on ${Build.MANUFACTURER} ${Build.MODEL}, " +
                    "API ${Build.VERSION.SDK_INT}, ABI ${Build.SUPPORTED_ABIS.firstOrNull()}, " +
                    "view ${view.width}x${view.height}px density ${view.resources.displayMetrics.density}",
            )
        }
    }
}
