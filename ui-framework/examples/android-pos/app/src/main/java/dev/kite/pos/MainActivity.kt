package dev.kite.pos

import android.app.Activity
import android.os.Bundle
import android.util.Log
import android.widget.Toast
import dev.kite.KiteView

/** Hosts the Kite POS register full screen. */
class MainActivity : Activity() {

    lateinit var kite: KiteView
        private set

    /** Completed sales as `[amount, method]`, newest last. */
    val sales = mutableListOf<List<String>>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        System.loadLibrary("kitepos")
        kite = KiteView(this)
        kite.callbackListener = KiteView.CallbackListener { name, args ->
            Log.i(TAG, "callback $name(${args.joinToString()})")
            if (name == "checkout") {
                sales += args.toList()
                // In a real store this would go to the payment terminal / back office.
                Toast.makeText(this, "Sale complete: $${args[0]} by ${args[1]}", Toast.LENGTH_SHORT).show()
            }
        }
        setContentView(kite)
        Log.i(TAG, "Kite POS started")
    }

    override fun onDestroy() {
        kite.release()
        super.onDestroy()
    }

    companion object {
        const val TAG = "KitePOS"
    }
}
