package cloud.lazarev.serverglass

import android.app.Activity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowInfoTracker
import kotlinx.coroutines.flow.map

/**
 * What the hinge is doing right now.
 *
 * A foldable is not just "sometimes a wide screen". Three things change independently and the
 * layout has to answer all of them:
 *
 *  - **Width.** Folded it is a phone, unfolded it is closer to a small tablet. That part is the
 *    ordinary size-class question.
 *  - **The hinge itself.** A `FoldingFeature` occupies real pixels. Content centred across it is
 *    split by a physical seam, so anything laid out over that band has to be avoided rather than
 *    merely resized.
 *  - **Posture.** Half-opened and laid flat (tabletop) is a genuinely different device: the top
 *    half is upright and readable, the bottom half faces the desk.
 *
 * Collapsing all of that into "is the window wide" is what makes most apps feel wrong on a fold.
 */
data class FoldState(
    /** Present only when a hinge intersects the window. */
    val feature: FoldingFeature? = null,
) {
    /** A vertical hinge splits the screen left/right — the case a two-pane layout should respect. */
    val isVerticalSeparating: Boolean
        get() = feature?.let {
            it.isSeparating && it.orientation == FoldingFeature.Orientation.VERTICAL
        } ?: false

    /** Half-opened with a horizontal hinge: the top half is the part someone is actually reading. */
    val isTabletop: Boolean
        get() = feature?.let {
            it.state == FoldingFeature.State.HALF_OPENED &&
                it.orientation == FoldingFeature.Orientation.HORIZONTAL
        } ?: false

    /** Width of the hinge in pixels, so a layout can leave that band empty. */
    val hingeWidthPx: Int
        get() = feature?.bounds?.width() ?: 0

    /** Left edge of the hinge in window pixels, for splitting panes along it. */
    val hingeCenterXPx: Int?
        get() = feature?.bounds?.let { if (it.width() > 0 || isVerticalSeparating) it.centerX() else null }
}

/**
 * Observe the current fold posture.
 *
 * `WindowInfoTracker` is lifecycle-aware and emits on every hinge change, so a device unfolding
 * mid-session re-composes without the activity being recreated — which is the other half of the
 * `configChanges` declaration in the manifest.
 */
@Composable
fun rememberFoldState(): State<FoldState> {
    val context = LocalContext.current
    val activity = context as Activity

    val flow = remember(activity) {
        WindowInfoTracker.getOrCreate(activity)
            .windowLayoutInfo(activity)
            .map { info ->
                FoldState(info.displayFeatures.filterIsInstance<FoldingFeature>().firstOrNull())
            }
    }

    return flow.collectAsStateWithLifecycle(initialValue = FoldState())
}
