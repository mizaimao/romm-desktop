package net.zhenningzhang.romm_desktop

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.webkit.JavascriptInterface
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

    /**
     * Every webview this activity has been given, oldest first.
     *
     * Not one reference, because there is more than one. Settings is a real
     * second window — `WebviewWindowBuilder` with its own settings.html — and
     * on Android that is a second WebView stacked in this same activity rather
     * than a separate task. Holding only the newest meant Back asked whichever
     * page happened to be built last, and holding only the first meant Back
     * asked the page underneath the one you were looking at. Either way
     * Settings could not be closed with Back.
     */
    private val webViews = mutableListOf<WebView>()

    override fun onWebViewCreate(webView: WebView) {
        webViews.add(webView)
        // The page's only way to reach Android. See Bridge.
        webView.addJavascriptInterface(Bridge(), "RommAndroid")
    }

    /**
     * The handful of Android things the settings page needs to reach.
     *
     * A JavaScript interface rather than a Tauri command, because a Tauri
     * command runs in Rust and Rust has no way to get at the Android context —
     * Tauri does not expose it (tauri-apps/tauri#13267). Kotlin has it for
     * free, so the shortest honest path from a button in the page to an Intent
     * is this.
     *
     * Only this app's own local pages run in this webview, so there is no third
     * party to expose it to.
     */
    private inner class Bridge {
        /** Whether the app can read the ES-DE folders. */
        @JavascriptInterface
        fun hasAllFilesAccess(): Boolean =
            Build.VERSION.SDK_INT < Build.VERSION_CODES.R ||
                Environment.isExternalStorageManager()

        /**
         * Show the system switch for All files access.
         *
         * There is no dialog an app can raise for this one and no way to grant
         * it in-process; the switch lives in system settings and the most any
         * app can do is open that screen. Without a button for it, dismissing
         * the prompt at launch left no way back — which is exactly what
         * happened.
         */
        @JavascriptInterface
        fun openAllFilesAccess() {
            runOnUiThread { openAllFilesScreen() }
        }
    }

    /**
     * The page the user is actually looking at.
     *
     * Newest first, skipping any that have been detached — a closed settings
     * window leaves its WebView in the list but off the view tree, and asking a
     * dead one would silently answer "no" and quit the app.
     */
    private fun topWebView(): WebView? =
        webViews.lastOrNull { it.isAttachedToWindow && it.isShown }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        askForFilesOnce()

        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                val wv = topWebView()
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
     * Offer the All files access toggle whenever it is not granted.
     *
     * The ES-DE library lives in ordinary folders — /storage/emulated/0/ES-DE
     * and /storage/emulated/0/ROMs — and since Android 11 no ordinary
     * permission opens them. MANAGE_EXTERNAL_STORAGE does, and it is granted by
     * a switch in system settings rather than by a dialog an app can raise, so
     * the most an app can do is take you to the switch.
     *
     * Every launch until it is granted, which is what the other frontends on
     * this kind of device do — ES-DE included. An earlier version asked once
     * and remembered, on the reasoning that throwing someone out to a settings
     * screen twice is rude; that was the wrong trade. Without this permission a
     * frontend cannot see the library at all, so a single missed prompt leaves
     * an app that looks broken and says nothing, and there is no in-app control
     * to find your way back to.
     *
     * The check is the grant itself, so it stops on its own the moment the
     * switch is on. Nothing is remembered and there is no state to get stuck.
     */
    private fun askForFilesOnce() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
        if (Environment.isExternalStorageManager()) return
        openAllFilesScreen()
    }

    /**
     * Open the All files access screen, from launch or from the settings page.
     *
     * Wrapped: the per-app screen is missing on some builds, and a crash would
     * be a far worse trade than a permission nobody was offered. The generic
     * list is the fallback, and giving up is fine.
     */
    private fun openAllFilesScreen() {
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
    }
}
