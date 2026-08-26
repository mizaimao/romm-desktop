package net.zhenningzhang.romm_desktop

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
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
        askForFilesOnce()

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

    /**
     * Offer the All files access toggle, once ever.
     *
     * The ES-DE library lives in ordinary folders — /storage/emulated/0/ES-DE
     * and /storage/emulated/0/ROMs — and since Android 11 no ordinary
     * permission opens them. MANAGE_EXTERNAL_STORAGE does, and it is granted by
     * a switch in system settings rather than by a dialog an app can raise, so
     * the most an app can do is take you to the switch.
     *
     * Once ever, and remembered, because this throws the user out to a settings
     * screen. Doing it on every launch until granted would punish anyone who
     * looked at it and decided no, and there is a route back: Settings -> Apps
     * -> RomM-Desktop -> All files access. The Library pane in the app says so.
     *
     * Not fatal if declined. Without it the ES-DE folders simply do not read,
     * and everything else — browsing, downloading, the server — is unaffected.
     */
    private fun askForFilesOnce() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
        if (Environment.isExternalStorageManager()) return

        val prefs = getSharedPreferences("romm", MODE_PRIVATE)
        if (prefs.getBoolean(ASKED, false)) return
        prefs.edit().putBoolean(ASKED, true).apply()

        // Wrapped: the per-app screen is missing on some builds, and a crash on
        // first launch would be a far worse trade than a permission nobody was
        // offered. The generic list is the fallback, and giving up is fine.
        try {
            startActivity(
                Intent(
                    Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                    Uri.parse("package:$packageName"),
                )
            )
        } catch (e: Exception) {
            try {
                startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
            } catch (e: Exception) {
                // No settings screen to offer. The app works without it.
            }
        }
    }

    private companion object {
        const val ASK = "window.__androidBack ? window.__androidBack() : false"
        const val ASKED = "asked_all_files"
    }
}
