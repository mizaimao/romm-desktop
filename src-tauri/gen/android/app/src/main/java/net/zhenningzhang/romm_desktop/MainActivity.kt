package net.zhenningzhang.romm_desktop

import android.os.Bundle
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
    /**
     * Turn off wry's own Back handling.
     *
     * It asks the WebView `canGoBack()` and finishes the activity when the
     * answer is no. This app is one page that never navigates, so the answer is
     * always no and every press quit to the launcher — from inside a platform,
     * a collection, the lightbox, anywhere.
     *
     * Pushing a history entry from JavaScript does not help, and that is worth
     * recording because it looks like it should: `history.pushState` moves
     * `history.length` to 2, but same-document entries do not reach the
     * WebView's back-forward list, so `canGoBack()` stays false and the page
     * never sees a `popstate`. Measured on the device over the DevTools
     * protocol — the press arrived, the counter stayed at zero.
     */
    override val handleBackNavigation = false

    /** Set once the webview exists, which is after `onCreate`. */
    private var webView: WebView? = null

    override fun onWebViewCreate(webView: WebView) {
        this.webView = webView
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                val wv = webView
                if (wv == null) {
                    // Pressed before the page exists. Nothing can have an
                    // opinion yet, so behave like any other app.
                    quit()
                    return
                }
                // The page answers "true" when it consumed the press — it
                // closed the lightbox, the help panel, or walked one level up.
                // Anything else, including a page that has not finished
                // loading and so has no such function, means there was nowhere
                // to go and Back should leave.
                //
                // Asynchronous, because `evaluateJavascript` is. The press is
                // already consumed by the time the answer arrives, so quitting
                // happens in the callback rather than after it.
                wv.evaluateJavascript(ASK) { handled ->
                    if (handled != "true") quit()
                }
            }

            /**
             * Hand the press back to the system.
             *
             * Disabling this callback first is what stops it catching its own
             * re-dispatch and looping forever.
             */
            private fun quit() {
                isEnabled = false
                onBackPressedDispatcher.onBackPressed()
                isEnabled = true
            }
        })
    }

    private companion object {
        const val ASK = "window.__androidBack ? window.__androidBack() : false"
    }
}
