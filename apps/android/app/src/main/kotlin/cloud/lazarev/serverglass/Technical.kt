package cloud.lazarev.serverglass

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.ui.draw.alpha
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.foundation.Canvas
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.sg_ffi.EntityView
import uniffi.sg_ffi.MetricGauge
import uniffi.sg_ffi.TargetSnapshot

/**
 * Every reading, for the person who wants them.
 *
 * The simple screen answers "is my server OK". This answers "which core, which disk, which
 * interface, how hot" — and until it existed, Android was the only platform where that question
 * had no answer at all: three tiles and a process list, with the network, the filesystems, the
 * per-core breakdown and the temperatures collected by the core and then thrown away.
 *
 * Ordered the way people triage: what is it doing, what is it doing it *with*, then the detail you
 * only want once something looks wrong. Deliberately the same order and the same section names as
 * the Apple apps, so moving between a phone and a desk is not relearning the dashboard.
 */
@Composable
fun TechnicalHostScreen(
    host: Host,
    model: CoreModel,
    modifier: Modifier = Modifier,
    onBack: (() -> Unit)? = null,
    onSimple: () -> Unit,
    onCommand: () -> Unit = {},
) {
    val snapshot = host.snapshot
    val format = { gauge: MetricGauge -> model.formatGauge(gauge) }

    LazyColumn(
        modifier
            .fillMaxSize()
            .background(Theme.background)
            .padding(horizontal = 14.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Row(
                Modifier.fillMaxWidth().padding(top = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (onBack != null) {
                    IconButton(onClick = onBack) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                            tint = Theme.primary,
                        )
                    }
                    Spacer(Modifier.width(2.dp))
                }
                Column(Modifier.weight(1f)) {
                    Text(
                        snapshot.displayName.ifEmpty { host.address },
                        color = Theme.primary,
                        fontSize = 17.sp,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        listOf(snapshot.distro, snapshot.kernel, snapshot.arch)
                            .filter { it.isNotEmpty() }
                            .joinToString(" · "),
                        color = Theme.tertiary,
                        fontSize = 11.sp,
                        maxLines = 1,
                    )
                }
                // The way back to the plain-language view. Without it the technical view is a
                // one-way door, and the preference that put you here is remembered.
                IconButton(onClick = onCommand) {
                    Icon(
                        Icons.Outlined.Terminal,
                        contentDescription = "Run a command on this server",
                        tint = Theme.secondary,
                    )
                }
                TextButton(onClick = onSimple) {
                    Text("Simple", color = Theme.info, fontSize = 13.sp)
                }
            }
        }

        (snapshot.state as? uniffi.sg_ffi.ConnectionState.Failed)?.let { failed ->
            item {
                Banner(
                    text = failed.message,
                    detail = if (failed.recoverable) {
                        "ServerGlass will keep retrying."
                    } else {
                        "This will not resolve on its own."
                    },
                    color = Theme.bad,
                )
            }
        }
        if (snapshot.sourceErrors.isNotEmpty()) {
            item {
                Banner(
                    text = "${snapshot.sourceErrors.size} collector(s) reported a problem",
                    detail = snapshot.sourceErrors.joinToString("\n"),
                    color = Theme.warn,
                )
            }
        }

        if (snapshot.gauges.isEmpty()) {
            item {
                Text(
                    "Collecting…",
                    color = Theme.secondary,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(vertical = 40.dp),
                )
            }
        } else {
            // Deliberately the same order as the Apple apps, and the same pairing: two panels
            // side by side once there is room for them, stacked when there is not.
            item { OverviewPanel(snapshot, format) }
            item {
                Pair(
                    first = { CpuPanel(snapshot, format, it) },
                    second = { MemoryPanel(snapshot, format, it) },
                )
            }
            item {
                Pair(
                    first = { NetworkPanel(snapshot, format, it) },
                    second = { DiskPanel(snapshot, format, it) },
                )
            }
            item { ProcessPanel(snapshot, model) }
            item { FilesystemPanel(snapshot, format) }
            item { SensorPanel(snapshot, format) }
            item { GroupPanel(snapshot, "Sockets & TCP", format) }
        }
        item { Spacer(Modifier.height(24.dp)) }
    }
}

// ---------------------------------------------------------------- panels

/**
 * Two panels side by side when there is room, stacked when there is not.
 *
 * The same 680dp threshold the Apple apps use, and measured rather than derived from a size class
 * for the same reason: the case that matters is a width that changes while the app is running —
 * an unfolding phone, an iPad entering Split View — and a measured width re-evaluates on every one
 * of those.
 */
@Composable
private fun Pair(
    first: @Composable (Modifier) -> Unit,
    second: @Composable (Modifier) -> Unit,
) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        if (maxWidth >= 680.dp) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                first(Modifier.weight(1f))
                second(Modifier.weight(1f))
            }
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                first(Modifier)
                second(Modifier)
            }
        }
    }
}

@Composable
private fun OverviewPanel(snapshot: TargetSnapshot, format: (MetricGauge) -> String) {
    Panel("Overview") {
        // A grid rather than a row: six rings do not fit across a phone, and the same view has to
        // work unfolded without a second layout existing.
        run {
            // The same readings the Apple overview shows, in the same order.
            val tiles = buildList {
                snapshot.gauge("cpu_usage")?.let {
                    add(Overview(it, format(it), "${snapshot.cpuCount} cores"))
                }
                snapshot.gauge("mem_usage")?.let {
                    add(Overview(it, format(it), snapshot.pair("mem_used", "mem_total", format)))
                }
                snapshot.gauge("disk_usage")?.let { add(Overview(it, format(it), "root")) }
                snapshot.gauge("swap_usage")?.let {
                    add(Overview(it, format(it), snapshot.pair("swap_used", "swap_total", format)))
                }
                snapshot.gauge("cpu_temp")?.let { add(Overview(it, format(it), "processor")) }
                snapshot.gauge("load1")?.let {
                    val five = snapshot.gauge("load5")?.value
                    val fifteen = snapshot.gauge("load15")?.value
                    add(
                        Overview(
                            it,
                            "%.2f".format(it.value),
                            if (five != null && fifteen != null) {
                                "%.2f · %.2f".format(five, fifteen)
                            } else {
                                null
                            },
                        ),
                    )
                }
                snapshot.gauge("uptime")?.let { add(Overview(it, format(it), null, ring = false)) }
            }

            AdaptiveGrid(minimum = 92.dp, horizontalSpacing = 4.dp, verticalSpacing = 12.dp, count = tiles.size) { index ->
                val tile = tiles[index]
                if (tile.ring) {
                    HeadlineRing(tile.gauge, tile.caption, tile.detail)
                } else {
                    // Uptime has no maximum, so it gets a number rather than a ring: a ring
                    // implies a proportion of something. The Apple overview does the same.
                    Column(
                        Modifier.fillMaxWidth(),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(7.dp),
                    ) {
                        Box(Modifier.size(76.dp), contentAlignment = Alignment.Center) {
                            Text(
                                tile.caption,
                                color = Theme.primary,
                                fontSize = 15.sp,
                                fontWeight = FontWeight.SemiBold,
                                fontFamily = FontFamily.Monospace,
                                textAlign = TextAlign.Center,
                                maxLines = 1,
                            )
                        }
                        Text(
                            tile.gauge.label,
                            color = Theme.primary,
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Medium,
                        )
                    }
                }
            }
        }
    }
}

/** One overview tile: a reading, its headline number, and the sentence under it. */
private data class Overview(
    val gauge: MetricGauge,
    val caption: String,
    val detail: String?,
    val ring: Boolean = true,
)

/** `used / total`, e.g. `49.4 GiB / 62.4 GiB`. */
private fun TargetSnapshot.pair(
    used: String,
    total: String,
    format: (MetricGauge) -> String,
): String? {
    val u = gauge(used) ?: return null
    val t = gauge(total) ?: return null
    return "${format(u)} / ${format(t)}"
}

@Composable
private fun CpuPanel(
    snapshot: TargetSnapshot,
    format: (MetricGauge) -> String,
    modifier: Modifier = Modifier,
) {
    val cores = snapshot.entities(ofKind = "cpu")
        .sortedBy { it.display.toIntOrNull() ?: 0 }

    Panel("CPU", subtitle = "${snapshot.cpuCount} logical", modifier = modifier) {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            StackedBar(
                listOf(
                    Segment("User", snapshot.gauge("cpu_user")?.value ?: 0.0, Theme.info),
                    Segment("System", snapshot.gauge("cpu_system")?.value ?: 0.0, Theme.warn),
                    Segment("I/O wait", snapshot.gauge("cpu_iowait")?.value ?: 0.0, Theme.bad),
                    Segment("Steal", snapshot.gauge("cpu_steal")?.value ?: 0.0, Color(0xFFB07BE0)),
                ),
            )

            AdaptiveGrid(
                minimum = 108.dp,
                horizontalSpacing = 10.dp,
                verticalSpacing = 6.dp,
                count = cores.size,
            ) { index ->
                CoreBar(cores[index].display, cores[index].gauge("usage"))
            }

            Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                listOf("procs_running", "procs_blocked", "ctx_switches").forEach { metric ->
                    snapshot.gauge(metric)?.let {
                        StatCell(it.label, format(it), Theme.primary, it.history, Modifier.weight(1f))
                    }
                }
            }
        }
    }
}

@Composable
private fun MemoryPanel(
    snapshot: TargetSnapshot,
    format: (MetricGauge) -> String,
    modifier: Modifier = Modifier,
) {
    Panel("Memory", modifier = modifier) {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            snapshot.gauge("mem_usage")?.let {
                CapacityRow("Physical", it, snapshot.pair("mem_used", "mem_total", format), format(it))
            }
            snapshot.gauge("swap_usage")?.let {
                CapacityRow("Swap", it, snapshot.pair("swap_used", "swap_total", format), format(it))
            }

            // The breakdown is deliberately a plain list of quantities: on a host running ZFS
            // these do not sum to the total (ARC is neither free nor counted as cached), so
            // rendering them as a stacked bar would draw a picture that is simply untrue.
            val breakdown = listOf("mem_available", "mem_free", "mem_cached", "mem_buffers")
                .mapNotNull { snapshot.gauge(it) }
            AdaptiveGrid(minimum = 118.dp, horizontalSpacing = 12.dp, verticalSpacing = 5.dp, count = breakdown.size) {
                KeyValueRow(breakdown[it].label, format(breakdown[it]))
            }
        }
    }
}

@Composable
private fun NetworkPanel(
    snapshot: TargetSnapshot,
    format: (MetricGauge) -> String,
    modifier: Modifier = Modifier,
) {
    // Ranked by combined traffic. Sorting on receive alone buries a send-heavy uplink below idle
    // interfaces, and the cap then hides it entirely.
    val interfaces = snapshot.entities(ofKind = "net")
        .filter { (it.value("rx_bytes") + it.value("tx_bytes")) > 0.0 }
        .sortedByDescending { it.value("rx_bytes") + it.value("tx_bytes") }

    Panel("Network", subtitle = shown(interfaces.size, cap = 6), modifier = modifier) {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                snapshot.gauge("net_rx")?.let {
                    StatCell("Download", format(it), Theme.good, it.history, Modifier.weight(1f), Icons.Filled.ArrowDownward)
                }
                snapshot.gauge("net_tx")?.let {
                    StatCell("Upload", format(it), Theme.info, it.history, Modifier.weight(1f), Icons.Filled.ArrowUpward)
                }
            }
            interfaces.take(6).forEach { device ->
                ThroughputRow(
                    name = device.display,
                    incoming = device.gauge("rx_bytes"),
                    outgoing = device.gauge("tx_bytes"),
                    outgoingColor = Theme.info,
                    format = format,
                )
            }
        }
    }
}

@Composable
private fun DiskPanel(
    snapshot: TargetSnapshot,
    format: (MetricGauge) -> String,
    modifier: Modifier = Modifier,
) {
    val disks = snapshot.entities(ofKind = "disk")
        .sortedByDescending { it.value("read_bytes") + it.value("write_bytes") }

    Panel("Disk I/O", subtitle = shown(disks.size, cap = 6), modifier = modifier) {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                snapshot.gauge("disk_read")?.let {
                    StatCell("Read", format(it), Theme.good, it.history, Modifier.weight(1f), Icons.Filled.ArrowDownward)
                }
                snapshot.gauge("disk_write")?.let {
                    StatCell("Write", format(it), Theme.warn, it.history, Modifier.weight(1f), Icons.Filled.ArrowUpward)
                }
            }
            disks.take(6).forEach { device ->
                ThroughputRow(
                    name = device.display,
                    incoming = device.gauge("read_bytes"),
                    outgoing = device.gauge("write_bytes"),
                    outgoingColor = Theme.warn,
                    format = format,
                )
            }
        }
    }
}

@Composable
private fun FilesystemPanel(snapshot: TargetSnapshot, format: (MetricGauge) -> String) {
    val mounts = snapshot.entities(ofKind = "fs")
        .sortedByDescending { it.value("usage") }
    if (mounts.isEmpty()) return

    Panel("Filesystems", subtitle = "${mounts.size} mounted") {
        AdaptiveGrid(minimum = 300.dp, horizontalSpacing = 18.dp, verticalSpacing = 10.dp, count = mounts.size) { index ->
            val mount = mounts[index]
            mount.gauge("usage")?.let { usage ->
                CapacityRow(
                    name = mount.display,
                    usage = usage,
                    detail = mount.gauge("used")?.let { used ->
                        mount.gauge("total")?.let { total -> "${format(used)} / ${format(total)}" }
                    },
                    usageText = format(usage),
                )
            }
        }
    }
}

/**
 * Temperatures, fans and power draw.
 *
 * Grouped by kind rather than by chip and hottest first, so the reading that matters is the one
 * you see without reading the list.
 */
@Composable
private fun SensorPanel(snapshot: TargetSnapshot, format: (MetricGauge) -> String) {
    val sensors = snapshot.entities(ofKind = "sensor")
    if (sensors.isEmpty()) return

    val temperatures = sensors.filter { it.gauge("temp") != null }
        .sortedByDescending { it.value("temp") }
    val fans = sensors.filter { it.gauge("fan") != null }
    val power = sensors.filter { it.gauge("power") != null }

    val readings = temperatures.map { it to "temp" } + fans.map { it to "fan" } +
        power.map { it to "power" }

    Panel("Temperature & power", subtitle = "${sensors.size} sensors") {
        AdaptiveGrid(minimum = 150.dp, horizontalSpacing = 18.dp, verticalSpacing = 5.dp, count = readings.size) { index ->
            val (sensor, metric) = readings[index]
            val gauge = sensor.gauge(metric)!!
            KeyValueRow(
                sensor.display,
                format(gauge),
                // The core says how worrying a reading is; a temperature is judged against the
                // chip's own critical point there rather than by a number written here twice.
                emphasis = if (metric == "temp") gauge.color() else Theme.primary,
            )
        }
    }
}

@Composable
private fun ProcessPanel(snapshot: TargetSnapshot, model: CoreModel) {
    if (snapshot.topProcesses.isEmpty()) return
    Panel("Processes", subtitle = "top ${snapshot.topProcesses.size}") {
        Column {
            Row(
                Modifier.fillMaxWidth().padding(bottom = 5.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                ColumnHeader("PID", 52.dp, TextAlign.End)
                Text(
                    "COMMAND",
                    color = Theme.tertiary,
                    fontSize = 8.5.sp,
                    fontWeight = FontWeight.Medium,
                    letterSpacing = 0.5.sp,
                    modifier = Modifier.weight(1f),
                )
                ColumnHeader("CPU", 100.dp, TextAlign.End)
                ColumnHeader("MEMORY", 72.dp, TextAlign.End)
            }

            snapshot.topProcesses.forEach { process ->
                Row(
                    Modifier.fillMaxWidth().padding(vertical = 2.5.dp),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        process.pid,
                        color = Theme.tertiary,
                        fontSize = 10.sp,
                        fontFamily = FontFamily.Monospace,
                        textAlign = TextAlign.End,
                        modifier = Modifier.width(52.dp),
                    )
                    Row(
                        Modifier.weight(1f),
                        horizontalArrangement = Arrangement.spacedBy(5.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            process.command,
                            color = Theme.primary,
                            fontSize = 10.5.sp,
                            fontWeight = FontWeight.Medium,
                            fontFamily = FontFamily.Monospace,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false),
                        )
                        // Uninterruptible sleep and zombies are worth flagging; sleeping and
                        // running are the normal states and a badge for them would be noise.
                        if (process.state == "D" || process.state == "Z") {
                            Text(
                                process.state,
                                color = Theme.warn,
                                fontSize = 8.sp,
                                fontWeight = FontWeight.Medium,
                                modifier = Modifier
                                    .clip(RoundedCornerShape(3.dp))
                                    .background(Theme.warn.copy(alpha = 0.15f))
                                    .padding(horizontal = 3.dp, vertical = 1.dp),
                            )
                        }
                    }
                    Row(
                        Modifier.width(100.dp),
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(Modifier.width(46.dp)) {
                            // Share of the whole machine and how worrying it is both come from
                            // the core: 100% of one core on a 20-core box is 5% of the host, and
                            // two UIs doing that arithmetic is two chances to get it wrong.
                            CapacityBar(
                                process.machineFraction,
                                Theme.health(process.severity),
                                height = 4.dp,
                            )
                        }
                        Text(
                            "%.1f%%".format(process.cpuPercent),
                            color = Theme.primary,
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Medium,
                            fontFamily = FontFamily.Monospace,
                            textAlign = TextAlign.End,
                            modifier = Modifier.width(48.dp),
                        )
                    }
                    Text(
                        model.format(process.memoryBytes, "B", true),
                        color = Theme.secondary,
                        fontSize = 10.sp,
                        fontFamily = FontFamily.Monospace,
                        textAlign = TextAlign.End,
                        modifier = Modifier.width(72.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun ColumnHeader(text: String, width: Dp, align: TextAlign) {
    Text(
        text,
        color = Theme.tertiary,
        fontSize = 8.5.sp,
        fontWeight = FontWeight.Medium,
        letterSpacing = 0.5.sp,
        textAlign = align,
        modifier = Modifier.width(width),
    )
}

/** Any group the core produced, rendered as plain label/value pairs. */
@Composable
private fun GroupPanel(
    snapshot: TargetSnapshot,
    title: String,
    format: (MetricGauge) -> String,
) {
    val group = snapshot.group(title) ?: return
    if (group.gauges.isEmpty()) return

    Panel(group.title) {
        // Twenty socket counters are numbers to read, not gauges to interpret.
        AdaptiveGrid(minimum = 150.dp, horizontalSpacing = 18.dp, verticalSpacing = 5.dp, count = group.gauges.size) { index ->
            val gauge = group.gauges[index]
            KeyValueRow(
                gauge.label,
                format(gauge),
                emphasis = if (gauge.metric == "tcp_retrans" && gauge.value > 0) Theme.warn else Theme.primary,
            )
        }
    }
}

/**
 * A failure worth interrupting the readings for.
 *
 * The Apple apps have shown these since the beginning; Android showed nothing at all, so a host
 * whose collectors were failing looked identical to one that was simply quiet.
 */
@Composable
private fun Banner(text: String, detail: String, color: Color) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .background(color.copy(alpha = 0.12f))
            .border(1.dp, color.copy(alpha = 0.35f), RoundedCornerShape(10.dp))
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        Text(text, color = Theme.primary, fontSize = 12.sp, fontWeight = FontWeight.Medium)
        Text(detail, color = Theme.secondary, fontSize = 11.sp)
    }
}

// ---------------------------------------------------------------- pieces
//
// Every measurement below is taken from the matching SwiftUI component in
// `apps/shared/ServerGlassUI/Components.swift`, down to the point size and the column width. The
// two dashboards are meant to be the same dashboard; a font a point larger here is how "the same"
// becomes "nearly the same" and then becomes two designs.

/** Mirrors `Panel` in DesignSystem.swift. */
@Composable
private fun Panel(
    title: String,
    subtitle: String? = null,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Column(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .background(Theme.panel)
            .border(1.dp, Theme.border, RoundedCornerShape(10.dp))
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                title.uppercase(),
                color = Theme.secondary,
                fontSize = 9.5.sp,
                fontWeight = FontWeight.Medium,
                letterSpacing = 0.8.sp,
            )
            subtitle?.let {
                Text(
                    it,
                    color = Theme.tertiary,
                    fontSize = 9.5.sp,
                    fontFamily = FontFamily.Monospace,
                )
            }
        }
        content()
    }
}

/** Mirrors `RingGauge` + `HeadlineRing`. */
@Composable
private fun HeadlineRing(gauge: MetricGauge, caption: String, detail: String?) {
    Column(
        Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(7.dp),
    ) {
        Box(Modifier.size(76.dp), contentAlignment = Alignment.Center) {
            Canvas(Modifier.fillMaxSize()) {
                val stroke = 6.dp.toPx()
                val inset = stroke / 2
                val arc = Size(size.width - stroke, size.height - stroke)
                drawArc(
                    color = Theme.track,
                    startAngle = -90f,
                    sweepAngle = 360f,
                    useCenter = false,
                    topLeft = Offset(inset, inset),
                    size = arc,
                    style = Stroke(width = stroke, cap = StrokeCap.Round),
                )
                gauge.fraction()?.let {
                    drawArc(
                        color = gauge.color(),
                        startAngle = -90f,
                        sweepAngle = (it * 360).toFloat(),
                        useCenter = false,
                        topLeft = Offset(inset, inset),
                        size = arc,
                        style = Stroke(width = stroke, cap = StrokeCap.Round),
                    )
                }
            }
            Text(
                caption,
                color = Theme.primary,
                fontSize = 15.sp,
                fontWeight = FontWeight.SemiBold,
                fontFamily = FontFamily.Monospace,
                maxLines = 1,
            )
        }
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(gauge.label, color = Theme.primary, fontSize = 10.sp, fontWeight = FontWeight.Medium)
            detail?.let { ShrinkingText(it) }
        }
    }
}

/**
 * A caption that shrinks to fit rather than losing its end.
 *
 * SwiftUI's `minimumScaleFactor(0.7)` in a composable. Without it the narrowest overview column
 * ellipsised — "530.5 MiB / 7.8" — which is worse than small text, because a truncated quantity
 * reads as a real one. Shrinks to 70% and no further; past that it would be unreadable and the
 * column is simply too narrow.
 */
@Composable
private fun ShrinkingText(text: String) {
    val full = 9.sp
    var size by remember(text) { mutableStateOf(full) }
    var settled by remember(text) { mutableStateOf(false) }

    Text(
        text,
        color = Theme.secondary,
        fontSize = size,
        fontFamily = FontFamily.Monospace,
        maxLines = 1,
        softWrap = false,
        textAlign = TextAlign.Center,
        // Drawn invisibly until it fits, so the reader never sees it snap from one size to
        // another on the first frame.
        modifier = Modifier.alpha(if (settled) 1f else 0f),
        onTextLayout = { layout ->
            if (!settled) {
                if (layout.hasVisualOverflow && size > full * 0.7f) {
                    size = size * 0.92f
                } else {
                    settled = true
                }
            }
        },
    )
}

/** Mirrors `CapacityBar`: 6dp by default, never thinner than 2dp of fill. */
@Composable
private fun CapacityBar(fraction: Double, color: Color, height: Dp = 6.dp) {
    Box(
        Modifier
            .fillMaxWidth()
            .height(height)
            .clip(CircleShape)
            .background(Theme.track),
    ) {
        val share = fraction.coerceIn(0.0, 1.0).toFloat()
        if (share > 0f) {
            Box(Modifier.fillMaxWidth(share).fillMaxHeight().clip(CircleShape).background(color))
        }
    }
}

/** Mirrors `CapacityRow`. */
@Composable
private fun CapacityRow(name: String, usage: MetricGauge, detail: String?, usageText: String) {
    Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                name,
                color = Theme.primary,
                fontSize = 11.sp,
                fontWeight = FontWeight.Medium,
                fontFamily = FontFamily.Monospace,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
            Spacer(Modifier.weight(1f))
            detail?.let {
                Text(
                    it,
                    color = Theme.secondary,
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                )
                Spacer(Modifier.width(8.dp))
            }
            Text(
                usageText,
                color = usage.color(),
                fontSize = 10.sp,
                fontWeight = FontWeight.SemiBold,
                fontFamily = FontFamily.Monospace,
                textAlign = TextAlign.End,
                modifier = Modifier.width(46.dp),
            )
        }
        CapacityBar(usage.fraction() ?: 0.0, usage.color())
    }
}

/** Mirrors `StatCell`. */
@Composable
private fun StatCell(
    label: String,
    value: String,
    color: Color,
    history: List<Double>,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(3.dp)) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(3.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            icon?.let { Icon(it, contentDescription = null, tint = color, modifier = Modifier.size(8.dp)) }
            Text(
                label.uppercase(),
                color = Theme.secondary,
                fontSize = 8.5.sp,
                fontWeight = FontWeight.Medium,
                letterSpacing = 0.5.sp,
                maxLines = 1,
            )
        }
        Text(
            value,
            color = color,
            fontSize = 13.sp,
            fontWeight = FontWeight.SemiBold,
            fontFamily = FontFamily.Monospace,
            maxLines = 1,
        )
        if (history.size > 1) {
            Sparkline(history, color, Modifier.fillMaxWidth().height(14.dp))
        }
    }
}

/** Mirrors `KeyValueRow`. */
@Composable
private fun KeyValueRow(label: String, value: String, emphasis: Color = Theme.primary) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(
            label,
            color = Theme.secondary,
            fontSize = 10.sp,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            modifier = Modifier.weight(1f),
        )
        Spacer(Modifier.width(4.dp))
        Text(
            value,
            color = emphasis,
            fontSize = 10.5.sp,
            fontWeight = FontWeight.Medium,
            fontFamily = FontFamily.Monospace,
        )
    }
}

/** Mirrors `CoreBar`. */
@Composable
private fun CoreBar(index: String, usage: MetricGauge?) {
    val percent = usage?.value ?: 0.0
    Row(
        horizontalArrangement = Arrangement.spacedBy(5.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            index,
            color = Theme.tertiary,
            fontSize = 8.5.sp,
            fontFamily = FontFamily.Monospace,
            textAlign = TextAlign.End,
            modifier = Modifier.width(16.dp),
        )
        Box(Modifier.weight(1f)) {
            CapacityBar(percent / 100, usage?.color() ?: Theme.good, height = 5.dp)
        }
        Text(
            "${Math.round(percent)}",
            color = Theme.secondary,
            fontSize = 8.5.sp,
            fontFamily = FontFamily.Monospace,
            textAlign = TextAlign.End,
            modifier = Modifier.width(18.dp),
        )
    }
}

/** One slice of the CPU stacked bar. */
private data class Segment(val label: String, val percent: Double, val color: Color)

/** Mirrors `StackedBar`. */
@Composable
private fun StackedBar(segments: List<Segment>) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(
            Modifier
                .fillMaxWidth()
                .height(8.dp)
                .clip(CircleShape)
                .background(Theme.track),
        ) {
            segments.forEach { segment ->
                val share = (segment.percent / 100.0).coerceIn(0.0, 1.0).toFloat()
                if (share > 0f) {
                    Box(Modifier.fillMaxHeight().weight(share).background(segment.color))
                }
            }
            val used = segments.sumOf { it.percent }.coerceIn(0.0, 100.0).toFloat() / 100f
            if (used < 1f) Box(Modifier.fillMaxHeight().weight(1f - used))
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            segments.forEach { segment ->
                Row(
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.size(5.dp).clip(CircleShape).background(segment.color))
                    Text(
                        segment.label,
                        color = Theme.secondary,
                        fontSize = 9.sp,
                        fontWeight = FontWeight.Medium,
                    )
                    Text(
                        "%.1f%%".format(segment.percent),
                        color = Theme.primary,
                        fontSize = 9.sp,
                        fontWeight = FontWeight.Medium,
                        fontFamily = FontFamily.Monospace,
                    )
                }
            }
        }
    }
}

/** Mirrors `InterfaceRow` and `DeviceRow`, which are the same row with different colours. */
@Composable
private fun ThroughputRow(
    name: String,
    incoming: MetricGauge?,
    outgoing: MetricGauge?,
    outgoingColor: Color,
    format: (MetricGauge) -> String,
) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            name,
            color = Theme.primary,
            fontSize = 10.5.sp,
            fontWeight = FontWeight.Medium,
            fontFamily = FontFamily.Monospace,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.width(78.dp),
        )
        incoming?.let {
            Sparkline(it.history, Theme.good, Modifier.weight(1f).height(14.dp))
            Text(
                format(it),
                color = Theme.good,
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                textAlign = TextAlign.End,
                modifier = Modifier.width(74.dp),
            )
        }
        outgoing?.let {
            Text(
                format(it),
                color = outgoingColor,
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                textAlign = TextAlign.End,
                modifier = Modifier.width(74.dp),
            )
        }
    }
}

/**
 * A grid that fills as many columns of at least `minimum` as fit.
 *
 * `LazyVGrid(columns: .adaptive(minimum:))` in one composable, so the two dashboards break into
 * the same number of columns at the same widths.
 */
@Composable
private fun AdaptiveGrid(
    minimum: Dp,
    horizontalSpacing: Dp,
    verticalSpacing: Dp,
    count: Int,
    item: @Composable (Int) -> Unit,
) {
    if (count == 0) return
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val columns = ((maxWidth + horizontalSpacing) / (minimum + horizontalSpacing))
            .toInt()
            .coerceAtLeast(1)
        Column(verticalArrangement = Arrangement.spacedBy(verticalSpacing)) {
            (0 until count).chunked(columns).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(horizontalSpacing)) {
                    row.forEach { index -> Box(Modifier.weight(1f)) { item(index) } }
                    repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- lookups

/**
 * These mirror the Swift helpers of the same names exactly.
 *
 * They are lookups into what the core already decided, not decisions of their own: the label, the
 * unit, the maximum and the history all arrive formed, and asking for them by metric name is the
 * whole of what a UI is allowed to do here.
 */
fun TargetSnapshot.allGauges(): List<MetricGauge> =
    gauges + detailGroups.flatMap { it.gauges }

fun TargetSnapshot.gauge(metric: String): MetricGauge? =
    allGauges().firstOrNull { it.metric == metric }

fun TargetSnapshot.entities(ofKind: String): List<EntityView> =
    entities.filter { it.kind == ofKind }

fun TargetSnapshot.group(title: String) =
    detailGroups.firstOrNull { it.title == title }

fun EntityView.gauge(metric: String): MetricGauge? =
    gauges.firstOrNull { it.metric == metric }

fun EntityView.value(metric: String): Double = gauge(metric)?.value ?: 0.0

/** 0–1 for a ring or a bar, absent when the reading has no maximum to be a proportion of. */
fun MetricGauge.fraction(): Double? = max?.takeIf { it > 0 }?.let { (value / it).coerceIn(0.0, 1.0) }

/**
 * The colour for this reading, from the level the core assigned it.
 *
 * The thresholds used to live here and in SwiftUI, and they had already drifted apart — 0.75/0.90
 * against 0.60/0.85 — so the same host was amber on a phone and green on a desk. A view layer maps
 * a level onto a colour; deciding what counts as "busy" is the core's job.
 */
fun MetricGauge.color(): Color = Theme.health(severity)

/** "6 of 14" when a list is capped, and nothing when everything is on screen. */
private fun shown(count: Int, cap: Int): String =
    if (count > cap) "$cap of $count" else "$count"
