package net.zhenningzhang.romm_desktop

import android.content.Intent
import android.graphics.Color
import android.net.Uri
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

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

    /** Launcher for the folder picker, and where to send the answer back. */
    private lateinit var folderPicker: ActivityResultLauncher<Uri?>
    private var folderPickerTarget: String? = null

    override fun onWebViewCreate(webView: WebView) {
        webViews.add(webView)
        // Paint the WebView itself, not just the page inside it.
        //
        // A WebView starts white and composites the page over it. This page is
        // dark and mostly opaque, but not entirely — the backdrop canvas and
        // the glass surfaces are drawn with alpha — so that white came through
        // everywhere as a flat wash. Measured: with the page painted pure
        // black, the screen showed #1f1f1f, a uniform twelve per cent of white
        // over the whole window. That is the tint, and no amount of stylesheet
        // work could have reached it, which is why three attempts in the CSS
        // changed nothing.
        webView.setBackgroundColor(Color.parseColor("#14161A"))
        stopDarkening(webView)
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

        /**
         * Whether RetroArch is installed, and which build.
         *
         * On Android RetroArch is a package, not a path — there is nothing to
         * browse for and nothing to point at. Either the app is on the device
         * or it is not, and the settings page should say which rather than
         * offering a file picker that cannot mean anything.
         *
         * Both ABIs are checked because a device may carry either.
         */
        /**
         * Open the system folder picker and hand the answer back to the page.
         *
         * Android has no directory dialog an app can call and await; it has an
         * activity that returns a result later. So this returns nothing, and
         * the answer arrives at `window.__folderPicked(target, path)` — the
         * target being whichever field asked, so two pickers cannot be
         * confused.
         */
        @JavascriptInterface
        fun pickFolder(target: String) {
            runOnUiThread {
                folderPickerTarget = target
                try {
                    folderPicker.launch(null)
                } catch (e: Exception) {
                    folderPickerTarget = null
                }
            }
        }

        @JavascriptInterface
        fun retroArchPackage(): String {
            for (pkg in arrayOf("com.retroarch.aarch64", "com.retroarch")) {
                try {
                    packageManager.getPackageInfo(pkg, 0)
                    return pkg
                } catch (e: Exception) {
                    // Not installed under that name; try the next.
                }
            }
            return ""
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
        goFullScreen()
        askForFilesOnce()

        // Registered here because a result launcher may only be created before
        // the activity is STARTED. The page asks for a folder later, by name,
        // and the name comes back with the answer so two pickers cannot be
        // confused for each other.
        folderPicker =
            registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
                val target = folderPickerTarget ?: return@registerForActivityResult
                folderPickerTarget = null
                val path = uri?.let { treeUriToPath(it) } ?: ""
                val js = "window.__folderPicked && window.__folderPicked(" +
                    org.json.JSONObject.quote(target) + "," +
                    org.json.JSONObject.quote(path) + ")"
                topWebView()?.evaluateJavascript(js, null)
            }

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
     * Hide the status bar and the gesture bar.
     *
     * This is a games launcher on a handheld; the clock, the battery and the
     * wifi bars belong to a phone. They also sat *over* the app rather than
     * above it — `enableEdgeToEdge` draws behind them — so the first tab was
     * underneath the clock and the last row of the list underneath the gesture
     * pill.
     *
     * BEHAVIOUR_SHOW_TRANSIENT_BARS_BY_SWIPE rather than hiding them outright:
     * a swipe from the edge brings them back for a few seconds and then they
     * leave again, so the time and the battery are still reachable while
     * nothing is permanently covering the library.
     *
     * Re-applied on focus because Android puts them back after a dialog, the
     * recents switcher, or the folder picker returning.
     */
    private fun goFullScreen() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        WindowInsetsControllerCompat(window, window.decorView).apply {
            hide(WindowInsetsCompat.Type.systemBars())
            systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            goFullScreen()
            paintWebViews()
        }
    }

    /**
     * Make every webview opaque, again.
     *
     * Setting this in `onWebViewCreate` is too early: wry configures the view
     * after handing it over, and Tauri supports transparent windows, so it puts
     * the background back to nothing. The result is a surface composited at
     * roughly 88% over white — which is arithmetic, not a guess: with the page
     * painted #14161A the device showed #313236, and 20*0.88 + 255*0.12 = 48.2,
     * which is 0x31. Every "tint" chased through the stylesheet was that white
     * coming through a page that could not be opaque no matter what it drew.
     */
    private fun paintWebViews() {
        val bg = Color.parseColor("#14161A")
        for (wv in webViews) {
            // An opaque colour is what makes a WebView opaque; `isOpaque` is
            // derived and read-only.
            wv.setBackgroundColor(bg)
            stopDarkening(wv)
        }
    }

    /**
     * Stop the WebView rewriting this page's colours.
     *
     * The device is in night mode, so the WebView applies its own darkening
     * pass on top of the page: it decides dark backgrounds are too dark and
     * lifts them toward a Material surface colour. This app is already dark and
     * did not ask.
     *
     * The effect is a flat wash over everything the stylesheet paints, and it
     * is invisible from inside the page — `getComputedStyle` still reports
     * rgb(20, 22, 26) while the device draws #313236. That is why it survived
     * being chased through the CSS: the CSS was right, and something after it
     * was changing the answer.
     *
     * Canvas pixels are exempt from the pass, which is why the backdrop looked
     * correct and everything around it did not, and why the wash appeared to
     * arrive when the cursor moved — moving it is what makes the canvas repaint
     * over the part that was washed.
     *
     * Two APIs because the name changed: `isAlgorithmicDarkeningAllowed` from
     * 33, `forceDark` before it.
     */
    @Suppress("DEPRECATION")
    private fun stopDarkening(wv: WebView) {
        // Both, not one or the other.
        //
        // The newer switch is the documented one and defaults to off for a
        // target this recent, so on paper neither is needed. The device says
        // otherwise: the page paints rgb(20, 22, 26) and the screen shows
        // #313236, which is that colour lifted by twelve per cent of white —
        // and canvas pixels, which this pass does not touch, stay dark in the
        // same frame. So the old switch is set too, and what actually took is
        // logged rather than assumed.
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                wv.settings.isAlgorithmicDarkeningAllowed = false
            }
        } catch (e: Exception) {
        }
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                wv.settings.forceDark = android.webkit.WebSettings.FORCE_DARK_OFF
            }
        } catch (e: Exception) {
        }
        // What actually took, once, because the two switches disagree: the
        // newer one reads back false as asked, and the older one still reads
        // AUTO because it is a no-op for a target this recent. Neither stopped
        // the wash, which is recorded here so the next person does not repeat
        // the experiment.
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

    /**
     * Turn what the folder picker hands back into a path the app can open.
     *
     * The picker answers with a tree URI — `content://com.android.externalstorage
     * .documents/tree/primary%3AGames%2FROMs` — and every path in this app is a
     * filesystem path. With All files access granted the two describe the same
     * place, so the URI is unwrapped rather than carried around: `primary` is
     * shared storage, and anything else is a card, mounted under /storage by
     * its volume id.
     *
     * Returns empty when the shape is not recognised, and the page says it
     * could not read that folder rather than storing a path that opens nothing.
     */
    private fun treeUriToPath(uri: Uri): String {
        val id = android.provider.DocumentsContract.getTreeDocumentId(uri) ?: return ""
        val parts = id.split(':', limit = 2)
        val volume = parts.getOrNull(0) ?: return ""
        val rest = parts.getOrNull(1).orEmpty()
        val root = if (volume.equals("primary", true)) {
            Environment.getExternalStorageDirectory().absolutePath
        } else {
            "/storage/$volume"
        }
        return if (rest.isEmpty()) root else "$root/$rest"
    }

    private companion object {
        const val ASK = "window.__androidBack ? window.__androidBack() : false"
    }
}
