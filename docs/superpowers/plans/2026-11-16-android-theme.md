# Android « unicorn » theme Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `docs/superpowers/specs/2026-11-16-android-theme-design.md`: fixed unicorn palette, gradient top bar under the status bar, night bottom bar, gradient screen headers, tabs as a 2-column grid with page thumbnails and favicons.

**Architecture:** `ui/Theme.kt` holds the palette, fixed dark/light `ColorScheme`s and a gradient `Brush`. `App.kt` stops applying the Scaffold's window insets and lets the top block (status bar) and bottom bar (navigation bar) handle them. `WebViews.kt` keeps per-tab thumbnails and favicons in Compose state. A new `ui/Tabs.kt` draws the grid.

**Tech Stack:** Kotlin, Jetpack Compose (BOM 2026.09.00, Material 3), Android WebView, JUnit 4 for JVM tests.

**Conventions (read first):**
- Windows machine. Gradle: `android\gradlew.bat` from the PowerShell tool, working directory `android`.
- The Kotlin bindings of the Rust core (`app/src/main/java/uniffi/`) are generated and git-ignored. If compilation fails with `Unresolved reference 'uniffi'`, generate them once (PowerShell, repo root): `cargo build -p mobile; cargo run -q -p mobile --features cli --bin uniffi-bindgen -- generate --library target\debug\mobile.dll --language kotlin --no-format --out-dir android\app\src\main\java`.
- Verify after every task: `.\gradlew.bat -q :app:compileDebugKotlin` (no `e:` lines). At the end also `.\gradlew.bat -q :app:testDebugUnitTest :app:lintVitalRelease`.
- One commit per task, conventional subject (`feat(android): …`), short English body if useful. The user is the only author: **no `Co-Authored-By` trailer**. Do not push.
- English comments in the existing style (short, explain why).

---

### Task 1: Palette, color schemes, contrast test

**Files:**
- Modify: `android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/Theme.kt` (whole file)
- Modify: `android/app/build.gradle.kts` (dependencies)
- Create: `android/app/src/test/java/io/github/ajustor/browsingunicorns/ui/ThemeTest.kt`

- [ ] **Step 1: Replace `Theme.kt`**

```kotlin
package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color

/** The icon's night purple and cyan, as on the desktop. */
object Unicorn {
    val GradientStart = Color(0xFF3A246E)
    val GradientMiddle = Color(0xFF5B3FA6)
    val GradientEnd = Color(0xFF2D8FB3)
    /** Bottom bar. */
    val Night = Color(0xFF241A3D)
    /** Address bar and its outline. */
    val NightDeep = Color(0xFF171126)
    val Outline = Color(0xFF3F3270)
    /** Text and icons on the bars. */
    val OnNight = Color(0xFFE9E4FF)
    val Cyan = Color(0xFF5ADCFF)

    val DarkBackground = Color(0xFF1C1430)
    val DarkSurface = Color(0xFF241A3D)
    val DarkHover = Color(0xFF2A1F47)
    val DarkText = Color(0xFFECE7FF)

    val LightBackground = Color(0xFFFAF8FF)
    val LightSurface = Color(0xFFEFEAFF)
    val LightAccent = Color(0xFF5B3FA6)
    val LightText = Color(0xFF241B3D)
}

/** The top bar's and headers' gradient (diagonal, left to right). */
val UnicornGradient = Brush.linearGradient(
    listOf(Unicorn.GradientStart, Unicorn.GradientMiddle, Unicorn.GradientEnd),
)

/** The loading bar: violet to cyan. */
val ProgressGradient = Brush.horizontalGradient(listOf(Color(0xFF8A5CFF), Unicorn.Cyan))

private val Dark = darkColorScheme(
    primary = Unicorn.Cyan,
    onPrimary = Unicorn.Night,
    secondary = Unicorn.GradientMiddle,
    onSecondary = Color.White,
    background = Unicorn.DarkBackground,
    onBackground = Unicorn.DarkText,
    surface = Unicorn.DarkBackground,
    onSurface = Unicorn.DarkText,
    surfaceVariant = Unicorn.DarkSurface,
    onSurfaceVariant = Color(0xFFB9B0D8),
    surfaceContainer = Unicorn.DarkSurface,
    surfaceContainerHigh = Unicorn.DarkHover,
    outline = Unicorn.Outline,
)

private val Light = lightColorScheme(
    primary = Unicorn.LightAccent,
    onPrimary = Color.White,
    secondary = Unicorn.GradientEnd,
    onSecondary = Color.White,
    background = Unicorn.LightBackground,
    onBackground = Unicorn.LightText,
    surface = Unicorn.LightBackground,
    onSurface = Unicorn.LightText,
    surfaceVariant = Unicorn.LightSurface,
    onSurfaceVariant = Color(0xFF5A4E80),
    surfaceContainer = Unicorn.LightSurface,
    surfaceContainerHigh = Color(0xFFE6E0F5),
    outline = Color(0xFFE0D9F3),
)

@Composable
fun BrowsingUnicornsTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, content = content)
}

/** WCAG contrast ratio of two opaque colors (1 to 21). */
fun contrast(a: Color, b: Color): Double {
    fun channel(v: Float): Double =
        if (v <= 0.04045f) v / 12.92 else Math.pow((v + 0.055) / 1.055, 2.4)
    fun luminance(c: Color) = 0.2126 * channel(c.red) + 0.7152 * channel(c.green) + 0.0722 * channel(c.blue)
    val (la, lb) = luminance(a) to luminance(b)
    return (maxOf(la, lb) + 0.05) / (minOf(la, lb) + 0.05)
}
```

- [ ] **Step 2: JUnit for JVM tests**

In `android/app/build.gradle.kts`, inside `dependencies { … }`, add at the end:

```kotlin
    testImplementation("junit:junit:4.13.2")
```

- [ ] **Step 3: Write `ThemeTest.kt`**

```kotlin
package io.github.ajustor.browsingunicorns.ui

import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertTrue
import org.junit.Test

class ThemeTest {
    private fun readable(text: Color, on: Color, min: Double) {
        val ratio = contrast(text, on)
        assertTrue("contrast $ratio < $min", ratio >= min)
    }

    @Test fun contrastOfKnownPairs() {
        assertTrue(Math.abs(contrast(Color.Black, Color.White) - 21.0) < 0.01)
    }

    @Test fun textIsReadableEverywhere() {
        readable(Unicorn.OnNight, Unicorn.Night, 4.5)
        readable(Unicorn.OnNight, Unicorn.NightDeep, 4.5)
        readable(Unicorn.DarkText, Unicorn.DarkBackground, 4.5)
        readable(Unicorn.DarkText, Unicorn.DarkSurface, 4.5)
        readable(Unicorn.LightText, Unicorn.LightBackground, 4.5)
        readable(Unicorn.LightText, Unicorn.LightSurface, 4.5)
        readable(Color.White, Unicorn.LightAccent, 4.5)
        // Header titles on the gradient's lightest end.
        readable(Color.White, Unicorn.GradientEnd, 3.0)
    }
}
```

- [ ] **Step 4: Run tests, compile**

Run (PowerShell, in `android`): `.\gradlew.bat -q :app:testDebugUnitTest`
Expected: success (no failures).

- [ ] **Step 5: Commit**

```bash
git add android/app/build.gradle.kts android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/Theme.kt android/app/src/test
git commit -m "feat(android): unicorn palette instead of the wallpaper's colors"
```

---

### Task 2: Gradient top bar, night bottom bar, gradient progress

**Files:**
- Modify: `android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/App.kt` (`App`, `BrowserScreen`, `AddressBar`, `BottomBar`)

- [ ] **Step 1: Scaffold without insets; each screen handles them**

In `App`, change `Scaffold(snackbarHost = { SnackbarHost(snackbar) }) { padding ->` to

```kotlin
    // The bars reach under the status and navigation bars: each screen pads
    // itself (BrowserScreen's top block and bottom bar, the headers).
    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        contentWindowInsets = WindowInsets(0, 0, 0, 0),
    ) { padding ->
```

Keep `Box(Modifier.fillMaxSize().padding(padding).imePadding())`.

- [ ] **Step 2: Top block in `BrowserScreen`**

Replace the start of `BrowserScreen`'s column:

```kotlin
        AddressBar(c)
        if (c.progress < 100) {
            LinearProgressIndicator(progress = { c.progress / 100f }, modifier = Modifier.fillMaxWidth())
        }
```

with

```kotlin
        Box(Modifier.fillMaxWidth().background(UnicornGradient).statusBarsPadding()) {
            AddressBar(c)
        }
        if (c.progress < 100) {
            Box(Modifier.fillMaxWidth().height(3.dp).background(Unicorn.Night)) {
                Box(
                    Modifier.fillMaxWidth(c.progress / 100f).height(3.dp).background(ProgressGradient),
                )
            }
        }
```

- [ ] **Step 3: Address bar as a dark pill**

In `AddressBar`, replace the `colors = TextFieldDefaults.colors(…)` argument with

```kotlin
        colors = TextFieldDefaults.colors(
            focusedContainerColor = Unicorn.NightDeep,
            unfocusedContainerColor = Unicorn.NightDeep,
            focusedTextColor = Unicorn.OnNight,
            unfocusedTextColor = Unicorn.OnNight,
            cursorColor = Unicorn.Cyan,
            focusedLeadingIconColor = Unicorn.OnNight,
            unfocusedLeadingIconColor = Unicorn.OnNight,
            focusedTrailingIconColor = Unicorn.OnNight,
            unfocusedTrailingIconColor = Unicorn.OnNight,
            focusedPlaceholderColor = Unicorn.OnNight.copy(alpha = 0.6f),
            unfocusedPlaceholderColor = Unicorn.OnNight.copy(alpha = 0.6f),
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
        ),
```

add to the `TextField` modifier chain (after `.padding(…)`) `.border(1.dp, Unicorn.Outline, RoundedCornerShape(24.dp))`, and tint the lock cyan: in the leading `Icon` for the secure case add `tint = if (c.state.activeSecure) Unicorn.Cyan else Unicorn.OnNight`.

- [ ] **Step 4: Night bottom bar**

In `BottomBar`, replace `Surface(tonalElevation = 3.dp) {` with

```kotlin
    Surface(color = Unicorn.Night, contentColor = Unicorn.OnNight) {
```

and make its `Row` modifier `Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 4.dp)`. The bookmarked star's tint becomes `Unicorn.Cyan`; the tab counter's border `BorderStroke(2.dp, Unicorn.OnNight)`. Disabled `IconButton`s already fade their content color.

- [ ] **Step 5: Imports**

Add the imports the steps need: `androidx.compose.foundation.background`, `androidx.compose.foundation.border`, `androidx.compose.foundation.layout.WindowInsets`, `androidx.compose.foundation.layout.statusBarsPadding`, `androidx.compose.foundation.layout.navigationBarsPadding`, `androidx.compose.foundation.layout.height`; remove `LinearProgressIndicator` if unused.

- [ ] **Step 6: Compile, commit**

Run: `.\gradlew.bat -q :app:compileDebugKotlin` (no `e:` lines).

```bash
git add android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/App.kt
git commit -m "feat(android): gradient top bar, night bottom bar"
```

---

### Task 3: Gradient headers, rows as cards

**Files:**
- Modify: `android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/Lists.kt` (`Header`, `PageRow`)
- Check: every screen using `Header` (`Lists.kt`, `Extensions.kt`, `SyncScreen.kt`) — no signature change, nothing to edit there.

- [ ] **Step 1: `Header`**

```kotlin
@Composable
fun Header(c: BrowserController, title: String, actions: @Composable () -> Unit = {}) {
    CompositionLocalProvider(LocalContentColor provides Color.White) {
        Row(
            Modifier.fillMaxWidth().background(UnicornGradient).statusBarsPadding().padding(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = { c.screen = Screen.Browser }) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back to the page")
            }
            Text(title, style = MaterialTheme.typography.titleLarge, color = Color.White, modifier = Modifier.weight(1f))
            actions()
        }
    }
}
```

`TextButton`s in `actions` (e.g. History's "Clear") take `primary` as text color: wrap the call site? No — instead pass `colors = ButtonDefaults.textButtonColors(contentColor = Color.White)` where a `TextButton` is used inside a `Header` (grep `Header(c,` and look at each `actions` lambda).

- [ ] **Step 2: `PageRow` as a card**

Wrap the row in a rounded surface:

```kotlin
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant,
        shape = RoundedCornerShape(12.dp),
        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 3.dp),
    ) {
        Row(
            Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) { /* the current content, unchanged */ }
    }
```

and delete the `HorizontalDivider()` calls that follow `PageRow` in the lists (grep `HorizontalDivider()` in `Lists.kt`; keep the ones that separate sections).

- [ ] **Step 3: Compile, commit**

Run: `.\gradlew.bat -q :app:compileDebugKotlin`.

```bash
git add android/app/src/main/java/io/github/ajustor/browsingunicorns/ui
git commit -m "feat(android): gradient headers, list rows as cards"
```

---

### Task 4: Thumbnails and favicons

**Files:**
- Modify: `android/app/src/main/java/io/github/ajustor/browsingunicorns/WebViews.kt`

- [ ] **Step 1: State and capture**

Add to `WebViews` (near `pages`):

```kotlin
    /** Each tab's page, reduced, as last seen (the tabs grid). Memory only. */
    val thumbnails = androidx.compose.runtime.mutableStateMapOf<ULong, androidx.compose.ui.graphics.ImageBitmap>()

    /** Each tab's favicon, as the page announced it. */
    val favicons = androidx.compose.runtime.mutableStateMapOf<ULong, androidx.compose.ui.graphics.ImageBitmap>()

    /** Captures [tab]'s page at a third of its size; nothing if it can't. */
    private fun capture(tab: ULong) {
        val view = views[tab] ?: return
        if (view.width <= 0 || view.height <= 0) return
        runCatching {
            val bitmap = Bitmap.createBitmap(view.width / 3, view.height / 3, Bitmap.Config.RGB_565)
            val canvas = android.graphics.Canvas(bitmap)
            canvas.scale(1f / 3f, 1f / 3f)
            // What is on screen, not the page's top.
            canvas.translate(-view.scrollX.toFloat(), -view.scrollY.toFloat())
            view.draw(canvas)
            thumbnails[tab] = bitmap.asImageBitmap()
        }
    }

    /** Captures the visible tab (before showing the tabs grid). */
    fun captureActive() {
        views.entries.firstOrNull { it.value.visibility == View.VISIBLE }?.let { capture(it.key) }
    }
```

with import `androidx.compose.ui.graphics.asImageBitmap`.

- [ ] **Step 2: Capture on tab switch, clean on close**

In `show(tab: ULong?)`, before the loop: `captureActive()`.
In `closeView`, add `thumbnails.remove(tab)` and `favicons.remove(tab)`.

- [ ] **Step 3: Favicons**

In `create()`'s `WebChromeClient`, add:

```kotlin
            override fun onReceivedIcon(view: WebView, icon: Bitmap?) {
                icon?.let { favicons[tab] = it.asImageBitmap() }
            }
```

- [ ] **Step 4: Compile, commit**

```bash
git add android/app/src/main/java/io/github/ajustor/browsingunicorns/WebViews.kt
git commit -m "feat(android): tab thumbnails and favicons"
```

---

### Task 5: Tabs grid

**Files:**
- Create: `android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/Tabs.kt`
- Modify: `android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/Lists.kt` (delete `TabsScreen`)
- Modify: `android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/App.kt` (tabs button captures first)

- [ ] **Step 1: Write `Tabs.kt`**

```kotlin
package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.ajustor.browsingunicorns.BrowserController
import io.github.ajustor.browsingunicorns.Screen

/** The open tabs as cards: title, favicon and how the page last looked. */
@Composable
fun TabsScreen(c: BrowserController) {
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        Header(c, "Tabs (${c.state.tabs.size})") {
            IconButton(onClick = {
                c.act { openTab(null) }
                c.screen = Screen.Browser
            }) { Icon(Icons.Filled.Add, contentDescription = "New tab") }
        }
        LazyVerticalGrid(
            columns = GridCells.Fixed(2),
            contentPadding = PaddingValues(8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.fillMaxSize().navigationBarsPadding(),
        ) {
            items(c.state.tabs, key = { it.id.toLong() }) { tab ->
                TabCard(
                    title = tab.title.ifBlank { tab.url ?: "New tab" },
                    host = tab.url?.let { android.net.Uri.parse(it).host } ?: "",
                    active = tab.id == c.state.active,
                    thumbnail = c.views.thumbnails[tab.id],
                    favicon = c.views.favicons[tab.id],
                    onOpen = {
                        c.act { activate(tab.id) }
                        c.screen = Screen.Browser
                    },
                    onClose = { c.act { closeTab(tab.id) } },
                )
            }
        }
    }
}

@Composable
private fun TabCard(
    title: String,
    host: String,
    active: Boolean,
    thumbnail: ImageBitmap?,
    favicon: ImageBitmap?,
    onOpen: () -> Unit,
    onClose: () -> Unit,
) {
    Surface(
        shape = RoundedCornerShape(14.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = if (active) BorderStroke(2.dp, Unicorn.Cyan) else null,
        modifier = Modifier.fillMaxWidth().clickable(onClick = onOpen),
    ) {
        Column {
            Row(Modifier.padding(start = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                Favicon(favicon, host, 16)
                Text(
                    title,
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f).padding(start = 6.dp),
                )
                IconButton(onClick = onClose) {
                    Icon(Icons.Filled.Close, contentDescription = "Close tab", modifier = Modifier.size(18.dp))
                }
            }
            Box(
                Modifier.fillMaxWidth().aspectRatio(3f / 4f).background(Unicorn.Night),
                contentAlignment = Alignment.Center,
            ) {
                if (thumbnail != null) {
                    Image(
                        thumbnail,
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        alignment = Alignment.TopCenter,
                        modifier = Modifier.fillMaxSize(),
                    )
                } else {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Favicon(favicon, host, 40)
                        Spacer(Modifier.height(8.dp))
                        Text(host, color = Unicorn.OnNight, style = MaterialTheme.typography.bodySmall, maxLines = 1)
                    }
                }
            }
        }
    }
}

/** The page's icon, or its domain's initial in a dot. */
@Composable
private fun Favicon(icon: ImageBitmap?, host: String, size: Int) {
    if (icon != null) {
        Image(icon, contentDescription = null, modifier = Modifier.size(size.dp).clip(RoundedCornerShape(4.dp)))
    } else {
        Box(
            Modifier.size(size.dp).clip(CircleShape).background(UnicornGradient),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                host.removePrefix("www.").take(1).uppercase(),
                color = Color.White,
                style = if (size >= 32) MaterialTheme.typography.titleLarge else MaterialTheme.typography.labelSmall,
            )
        }
    }
}
```

Note: check the field types of the tab records (`c.state.tabs` items: `id: ULong`, `title: String`, `url: String?`) in the generated `uniffi/mobile/mobile.kt` (`data class TabInfo`); adapt if they differ.

- [ ] **Step 2: Remove the old `TabsScreen` from `Lists.kt`** (the whole function and imports it alone used).

- [ ] **Step 3: Capture before opening the grid**

In `App.kt`'s `BottomBar`, the tabs button: `IconButton(onClick = { c.views.captureActive(); c.screen = Screen.Tabs }) {`.

- [ ] **Step 4: Compile, tests, lint, commit**

Run: `.\gradlew.bat -q :app:compileDebugKotlin :app:testDebugUnitTest :app:lintVitalRelease`
Expected: no `e:` lines, tests pass, lint passes.

```bash
git add android/app/src/main/java/io/github/ajustor/browsingunicorns
git commit -m "feat(android): tabs as a grid of cards with page previews"
```
