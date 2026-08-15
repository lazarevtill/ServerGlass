package cloud.lazarev.serverglass

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.ui.graphics.Color
import androidx.compose.material3.windowsizeclass.ExperimentalMaterial3WindowSizeClassApi
import androidx.compose.material3.windowsizeclass.WindowWidthSizeClass
import androidx.compose.material3.windowsizeclass.calculateWindowSizeClass
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val demoHost = intent?.getStringExtra("host")
        val demoKey = intent?.getStringExtra("key")

        setContent {
            MaterialTheme(
                colorScheme = darkColorScheme(
                    // Material's default is purple. Every control in the app should be the same
                    // green the gauges are, or the buttons look like they belong to another app.
                    primary = Theme.good,
                    onPrimary = Color(0xFF06210F),
                    secondaryContainer = Theme.good.copy(alpha = 0.22f),
                    onSecondaryContainer = Theme.primary,
                    surface = Theme.background,
                    onSurface = Theme.primary,
                    surfaceVariant = Theme.panel,
                    onSurfaceVariant = Theme.secondary,
                    background = Theme.background,
                    onBackground = Theme.primary,
                ),
            ) {
                Surface(Modifier.fillMaxSize(), color = Theme.background) {
                    // `enableEdgeToEdge` lets the app draw behind the system bars, which is what
                    // makes the background reach the screen edges — but content still has to be
                    // inset out from under them, or the status bar clock lands on top of the
                    // health card. Applied here so both panes inherit it.
                    Box(Modifier.windowInsetsPadding(WindowInsets.systemBars)) {
                        App(demoHost = demoHost, demoKey = demoKey)
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3WindowSizeClassApi::class)
@Composable
fun App(model: CoreModel = viewModel(), demoHost: String? = null, demoKey: String? = null) {
    LaunchedEffect(demoHost) {
        if (demoHost != null) model.addDemoHost(demoHost, demoKey)
    }

    val windowSize = calculateWindowSizeClass(LocalContext.current as android.app.Activity)
    val fold by rememberFoldState()

    // Two panes when there is genuine width for them. A folded phone is Compact and gets one pane;
    // an unfolded Pixel Fold is Expanded and gets two — and because the activity survives the fold
    // (see `configChanges` in the manifest) that transition is a re-layout, not a restart.
    val twoPane = windowSize.widthSizeClass != WindowWidthSizeClass.Compact

    val selected = model.host(model.selection) ?: model.hosts.firstOrNull()
    var addingServer by remember { mutableStateOf(false) }

    // Two panes always show a detail, so the list highlight should follow it. One pane must not
    // select anything on its own — see the note in CoreModel.start.
    LaunchedEffect(twoPane, model.hosts.size) {
        if (twoPane && model.selection == null) model.selection = model.hosts.firstOrNull()?.id
    }

    // Without this, system back on a phone quits the app from the detail screen instead of
    // returning to the list.
    BackHandler(enabled = !twoPane && model.selection != null) { model.selection = null }

    if (twoPane && model.hosts.isNotEmpty()) {
        TwoPane(model = model, fold = fold, selected = selected, onAdd = { addingServer = true })
    } else {
        Box(Modifier.fillMaxSize().background(Theme.background)) {
            if (selected != null && model.selection != null) {
                SimpleHostScreen(
                    host = selected,
                    model = model,
                    onBack = { model.selection = null },
                )
            } else {
                HostList(model = model, onAdd = { addingServer = true })
            }
        }
    }

    if (addingServer) {
        AddServerDialog(model = model, onDismiss = { addingServer = false })
    }
}

/**
 * List and detail side by side, with the hinge treated as an obstacle rather than a suggestion.
 *
 * When a vertical hinge separates the two halves, the panes are split *at the hinge* and the seam
 * itself is left empty. Laying a single scrolling column across it — which is what a plain
 * `Row(weight(1f))` does — puts text under the fold, where on real hardware it is bent, dimmed, or
 * physically interrupted. Splitting to the hinge is the whole difference between an app that
 * tolerates a foldable and one that fits it.
 */
@Composable
private fun TwoPane(model: CoreModel, fold: FoldState, selected: Host?, onAdd: () -> Unit) {
    val density = LocalDensity.current

    Row(Modifier.fillMaxSize().background(Theme.background)) {
        val hingeCenter = fold.hingeCenterXPx?.takeIf { fold.isVerticalSeparating }

        if (hingeCenter != null) {
            val listWidth = with(density) { hingeCenter.toDp() } -
                with(density) { (fold.hingeWidthPx / 2).toDp() }
            val hingeWidth = with(density) { fold.hingeWidthPx.toDp() }

            HostList(model = model, onAdd = onAdd, modifier = Modifier.width(listWidth).fillMaxHeight())
            // The seam. Deliberately empty.
            Spacer(Modifier.width(hingeWidth).fillMaxHeight())
            DetailPane(model = model, selected = selected, modifier = Modifier.weight(1f))
        } else {
            HostList(model = model, onAdd = onAdd, modifier = Modifier.width(300.dp).fillMaxHeight())
            DetailPane(model = model, selected = selected, modifier = Modifier.weight(1f))
        }
    }
}

@Composable
private fun DetailPane(model: CoreModel, selected: Host?, modifier: Modifier = Modifier) {
    Box(modifier.fillMaxSize().background(Theme.background)) {
        if (selected != null) {
            SimpleHostScreen(host = selected, model = model)
        } else {
            androidx.compose.material3.Text(
                "Select a server",
                color = Theme.secondary,
                modifier = Modifier.padding(20.dp),
            )
        }
    }
}
