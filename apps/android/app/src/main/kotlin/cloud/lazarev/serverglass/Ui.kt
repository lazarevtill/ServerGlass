package cloud.lazarev.serverglass

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.sg_ffi.HostHealth
import uniffi.sg_ffi.SimpleTile

/**
 * The palette, matching the Apple apps exactly.
 *
 * Same rule as everywhere else in ServerGlass: the widget must match the metric. A ring implies a
 * proportion, so anything without a maximum does not get one.
 */
object Theme {
    val background = Color(0xFF0B0B0D)
    val panel = Color(0xFF15151A)
    /// Slightly lifted from `panel`, for the larger simple-view cards. One flat surface colour at
    /// every size makes big cards read as empty space.
    val card = Color(0xFF19191D)
    val border = Color(0x0FFFFFFF)
    val track = Color(0x14FFFFFF)

    val primary = Color(0xEBFFFFFF)
    val secondary = Color(0x73FFFFFF)
    val tertiary = Color(0x47FFFFFF)

    val good = Color(0xFF59D68C)
    val warn = Color(0xFFFABF47)
    val bad = Color(0xFFF77070)
    val info = Color(0xFF6BA8F7)

    fun health(level: String): Color = when (level) {
        "ok" -> good
        "busy" -> warn
        "problem", "offline" -> bad
        else -> secondary
    }
}

/**
 * A ring gauge, drawn only for readings that have a maximum.
 *
 * Deliberately the same shape, weight and colour rule as the Apple apps: a person moving between
 * their phone and their desk should not have to relearn the dashboard.
 */
@Composable
fun RingGauge(
    fraction: Double?,
    color: Color,
    label: String,
    diameter: Dp,
    modifier: Modifier = Modifier,
) {
    Box(modifier.size(diameter), contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            val stroke = if (diameter > 90.dp) 8.dp.toPx() else 6.dp.toPx()
            val inset = stroke / 2
            val arcSize = Size(size.width - stroke, size.height - stroke)
            drawArc(
                color = Theme.track,
                startAngle = -90f,
                sweepAngle = 360f,
                useCenter = false,
                topLeft = Offset(inset, inset),
                size = arcSize,
                style = Stroke(width = stroke, cap = StrokeCap.Round),
            )
            fraction?.let {
                drawArc(
                    color = color,
                    startAngle = -90f,
                    sweepAngle = (it.coerceIn(0.0, 1.0) * 360).toFloat(),
                    useCenter = false,
                    topLeft = Offset(inset, inset),
                    size = arcSize,
                    style = Stroke(width = stroke, cap = StrokeCap.Round),
                )
            }
        }
        Text(
            label,
            color = Theme.primary,
            fontSize = if (diameter > 90.dp) 21.sp else 16.sp,
            fontWeight = FontWeight.SemiBold,
            fontFamily = FontFamily.Monospace,
            maxLines = 1,
        )
    }
}

/**
 * A trend line over the recent window.
 *
 * Scaled to the observed range so small real movement is visible — but the span is floored at a
 * fraction of the magnitude, because storage ticking from 5.19% to 5.20% stretched to full height
 * draws a cliff and tells the reader the disk just filled up.
 */
@Composable
fun Sparkline(values: List<Double>, color: Color, modifier: Modifier = Modifier) {
    if (values.size < 2) return
    Canvas(modifier) {
        val lowest = values.min()
        val highest = values.max()
        val magnitude = maxOf(kotlin.math.abs(highest), kotlin.math.abs(lowest))
        val span = maxOf(highest - lowest, magnitude * 0.05)

        val points = values.mapIndexed { index, value ->
            val x = size.width * index / (values.size - 1).toFloat()
            val normalised = if (span > 0) ((value - lowest) / span) else 0.5
            Offset(x, size.height - (normalised * size.height).toFloat())
        }

        // A faint fill under the line: at this height a 1px stroke alone has no shape to read.
        val fill = Path().apply {
            moveTo(points.first().x, size.height)
            points.forEach { lineTo(it.x, it.y) }
            lineTo(points.last().x, size.height)
            close()
        }
        drawPath(fill, color.copy(alpha = 0.14f))

        val line = Path().apply {
            moveTo(points.first().x, points.first().y)
            points.drop(1).forEach { lineTo(it.x, it.y) }
        }
        drawPath(line, color.copy(alpha = 0.9f), style = Stroke(width = 1.6.dp.toPx()))
    }
}

@Composable
fun HealthCard(health: HostHealth, name: String, modifier: Modifier = Modifier) {
    val tint = Theme.health(health.level)
    Column(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(16.dp))
            // A gradient rather than a flat wash: at this size a solid block of colour reads as an
            // error banner even when it is green.
            .background(
                Brush.linearGradient(
                    listOf(tint.copy(alpha = 0.16f), tint.copy(alpha = 0.05f)),
                ),
            )
            .border(1.dp, tint.copy(alpha = 0.28f), RoundedCornerShape(16.dp))
            .padding(16.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.size(14.dp).clip(CircleShape).background(tint),
            )
            Spacer(Modifier.width(11.dp))
            Text(
                health.headline,
                color = Theme.primary,
                fontSize = 22.sp,
                fontWeight = FontWeight.SemiBold,
            )
        }
        if (health.detail.isNotEmpty()) {
            Spacer(Modifier.height(4.dp))
            Text(health.detail, color = Theme.secondary, fontSize = 13.sp)
        }
        Spacer(Modifier.height(3.dp))
        Text(name, color = Theme.tertiary, fontSize = 11.sp)
    }
}

@Composable
fun SimpleTileCard(tile: SimpleTile, ring: Dp = 104.dp, modifier: Modifier = Modifier) {
    Column(
        modifier
            .clip(RoundedCornerShape(16.dp))
            .background(Theme.card)
            .border(1.dp, Theme.border, RoundedCornerShape(16.dp))
            .padding(if (ring > 90.dp) 14.dp else 11.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Row(Modifier.fillMaxWidth()) {
            Text(
                tile.name,
                color = Theme.secondary,
                fontSize = 12.5.sp,
                fontWeight = FontWeight.Medium,
            )
        }
        Spacer(Modifier.height(11.dp))

        RingGauge(
            fraction = tile.fraction,
            color = Theme.health(tile.level),
            label = tile.valueText,
            diameter = ring,
        )

        Spacer(Modifier.height(11.dp))
        // Two lines reserved so all three cards end at the same height whether the summary is
        // "Barely working" or "240.9 GiB free of 254.2 GiB".
        Text(
            tile.summary,
            color = Theme.tertiary,
            fontSize = if (ring > 90.dp) 11.5.sp else 10.sp,
            textAlign = TextAlign.Center,
            minLines = 2,
            maxLines = 2,
            modifier = Modifier.fillMaxWidth(),
        )

        if (tile.history.size > 1) {
            Spacer(Modifier.height(9.dp))
            Sparkline(
                values = tile.history,
                color = Theme.health(tile.level),
                modifier = Modifier.fillMaxWidth().height(22.dp),
            )
        }
    }
}

/**
 * The default screen, matching the Apple apps: a verdict, four readings that mean something
 * without training, and what is keeping the machine busy.
 */
@Composable
fun SimpleHostScreen(host: Host, model: CoreModel, modifier: Modifier = Modifier) {
    val snapshot = host.snapshot
    val name = snapshot.displayName.ifEmpty { host.address }

    LazyColumn(
        modifier
            .fillMaxSize()
            .background(Theme.background)
            .padding(horizontal = 14.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        item { Spacer(Modifier.height(6.dp)) }
        item { HealthCard(snapshot.health, name) }

        if (snapshot.simpleTiles.isEmpty()) {
            item {
                Box(Modifier.fillMaxWidth().height(180.dp), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(color = Theme.secondary)
                }
            }
        } else {
            item {
                // Always one row of three, with the ring sized to the width available. A wrapping
                // grid put two tiles on the first row and left a hole beside the third.
                BoxWithConstraints(Modifier.fillMaxWidth()) {
                    val wide = maxWidth > 520.dp
                    Row(horizontalArrangement = Arrangement.spacedBy(if (wide) 12.dp else 9.dp)) {
                        snapshot.simpleTiles.forEach { tile ->
                            SimpleTileCard(
                                tile = tile,
                                ring = if (wide) 104.dp else 78.dp,
                                modifier = Modifier.weight(1f),
                            )
                        }
                    }
                }
            }
        }

        if (snapshot.topProcesses.isNotEmpty()) {
            item {
                Text(
                    "What's keeping it busy",
                    color = Theme.primary,
                    fontSize = 13.sp,
                    fontWeight = FontWeight.SemiBold,
                )
            }
            item {
                Column(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(12.dp))
                        .background(Theme.panel)
                        .border(1.dp, Theme.border, RoundedCornerShape(12.dp))
                        .padding(horizontal = 12.dp),
                ) {
                    snapshot.topProcesses.take(5).forEachIndexed { index, process ->
                        Row(
                            Modifier.fillMaxWidth().padding(vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                process.command,
                                color = Theme.primary,
                                fontSize = 13.sp,
                                modifier = Modifier.weight(1f),
                                maxLines = 1,
                            )
                            Text(
                                "%.0f%%".format(process.cpuPercent),
                                color = Theme.secondary,
                                fontSize = 13.sp,
                                fontFamily = FontFamily.Monospace,
                            )
                        }
                        if (index < snapshot.topProcesses.take(5).lastIndex) {
                            HorizontalDivider(color = Theme.border)
                        }
                    }
                }
            }
            item {
                Text(
                    "These are the programs using the most processing power right now.",
                    color = Theme.tertiary,
                    fontSize = 11.sp,
                )
            }
        }
        item { Spacer(Modifier.height(20.dp)) }
    }
}

/** The host list, used as the single pane when folded and the left pane when open. */
@Composable
fun HostList(model: CoreModel, modifier: Modifier = Modifier) {
    LazyColumn(
        modifier.fillMaxSize().background(Theme.background).padding(horizontal = 12.dp),
    ) {
        item {
            Text(
                "ServerGlass",
                color = Theme.primary,
                fontSize = 24.sp,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(vertical = 14.dp),
            )
        }
        items(model.hosts) { host ->
            val selected = host.id == model.selection
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(vertical = 4.dp)
                    .clip(RoundedCornerShape(10.dp))
                    .background(if (selected) Theme.panel else Color.Transparent)
                    .border(
                        1.dp,
                        if (selected) Theme.border else Color.Transparent,
                        RoundedCornerShape(10.dp),
                    )
                    .padding(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    Modifier
                        .size(8.dp)
                        .clip(CircleShape)
                        .background(Theme.health(host.snapshot.health.level)),
                )
                Spacer(Modifier.width(10.dp))
                Column(Modifier.weight(1f)) {
                    Text(
                        host.snapshot.displayName.ifEmpty { host.address },
                        color = Theme.primary,
                        fontSize = 14.sp,
                        fontWeight = FontWeight.Medium,
                        maxLines = 1,
                    )
                    Text(
                        host.snapshot.health.headline,
                        color = Theme.secondary,
                        fontSize = 11.sp,
                        maxLines = 1,
                    )
                }
            }
        }
        if (model.hosts.isEmpty()) {
            item {
                Text(
                    "No servers yet.",
                    color = Theme.secondary,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(12.dp),
                )
            }
        }
    }
}
