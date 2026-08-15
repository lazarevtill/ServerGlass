package cloud.lazarev.serverglass

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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
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
            item { OverviewPanel(snapshot, format, model) }
            item { CpuPanel(snapshot, format) }
            item { MemoryPanel(snapshot, format) }
            item { NetworkPanel(snapshot, format) }
            item { DiskPanel(snapshot, format) }
            item { FilesystemPanel(snapshot, format) }
            item { SensorPanel(snapshot, format) }
            item { ProcessPanel(snapshot, model) }
            item { GroupPanel(snapshot, "Sockets & TCP", format) }
        }
        item { Spacer(Modifier.height(24.dp)) }
    }
}

// ---------------------------------------------------------------- panels

@Composable
private fun OverviewPanel(
    snapshot: TargetSnapshot,
    format: (MetricGauge) -> String,
    model: CoreModel,
) {
    Panel("Overview") {
        // A grid rather than a row: six rings do not fit across a phone, and the same view has to
        // work unfolded without a second layout existing.
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val columns = (maxWidth / 108.dp).toInt().coerceIn(2, 6)
            val rings = listOf("cpu_usage", "mem_usage", "disk_usage", "swap_usage", "cpu_temp")
                .mapNotNull { snapshot.gauge(it) }

            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                rings.chunked(columns).forEach { row ->
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        row.forEach { gauge ->
                            Column(
                                Modifier.weight(1f),
                                horizontalAlignment = Alignment.CenterHorizontally,
                            ) {
                                RingGauge(
                                    fraction = gauge.fraction(),
                                    color = gauge.severity(),
                                    label = format(gauge),
                                    diameter = 74.dp,
                                )
                                Spacer(Modifier.height(6.dp))
                                Text(
                                    gauge.label,
                                    color = Theme.secondary,
                                    fontSize = 11.sp,
                                    maxLines = 1,
                                )
                            }
                        }
                        // Keeps a short last row aligned with the one above instead of centred.
                        repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                    }
                }
            }
        }
    }
}

@Composable
private fun CpuPanel(snapshot: TargetSnapshot, format: (MetricGauge) -> String) {
    val cores = snapshot.entities(ofKind = "cpu")
        .sortedBy { it.display.filter(Char::isDigit).toIntOrNull() ?: 0 }

    Panel("CPU", subtitle = "${snapshot.cpuCount} cores") {
        Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
            snapshot.group("CPU")?.gauges?.forEach { gauge ->
                KeyValueRow(gauge.label, format(gauge))
            }
            if (cores.isNotEmpty()) {
                Spacer(Modifier.height(4.dp))
                cores.forEach { core ->
                    val usage = core.gauge("usage")
                    if (usage != null) {
                        CoreBar(core.display, usage, format(usage))
                    }
                }
            }
        }
    }
}

@Composable
private fun MemoryPanel(snapshot: TargetSnapshot, format: (MetricGauge) -> String) {
    Panel("Memory") {
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            snapshot.group("Memory")?.gauges?.forEach { gauge ->
                KeyValueRow(gauge.label, format(gauge))
            }
        }
    }
}

@Composable
private fun NetworkPanel(snapshot: TargetSnapshot, format: (MetricGauge) -> String) {
    // Ranked by combined traffic. Sorting on receive alone buries a send-heavy uplink below idle
    // interfaces, and the cap then hides it entirely.
    val interfaces = snapshot.entities(ofKind = "net")
        .filter { (it.value("rx_bytes") + it.value("tx_bytes")) > 0.0 }
        .sortedByDescending { it.value("rx_bytes") + it.value("tx_bytes") }

    Panel("Network", subtitle = shown(interfaces.size, cap = 6)) {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                snapshot.gauge("net_rx")?.let {
                    StatCell("Download", format(it), Theme.good, it.history, Modifier.weight(1f))
                }
                snapshot.gauge("net_tx")?.let {
                    StatCell("Upload", format(it), Theme.info, it.history, Modifier.weight(1f))
                }
            }
            interfaces.take(6).forEach { device ->
                ThroughputRow(
                    name = device.display,
                    down = device.gauge("rx_bytes")?.let(format),
                    up = device.gauge("tx_bytes")?.let(format),
                )
            }
        }
    }
}

@Composable
private fun DiskPanel(snapshot: TargetSnapshot, format: (MetricGauge) -> String) {
    val disks = snapshot.entities(ofKind = "disk")
        .sortedByDescending { it.value("read_bytes") + it.value("write_bytes") }

    Panel("Disk I/O", subtitle = shown(disks.size, cap = 6)) {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                snapshot.gauge("disk_read")?.let {
                    StatCell("Read", format(it), Theme.good, it.history, Modifier.weight(1f))
                }
                snapshot.gauge("disk_write")?.let {
                    StatCell("Write", format(it), Theme.warn, it.history, Modifier.weight(1f))
                }
            }
            disks.take(6).forEach { device ->
                ThroughputRow(
                    name = device.display,
                    down = device.gauge("read_bytes")?.let(format),
                    up = device.gauge("write_bytes")?.let(format),
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
        Column(verticalArrangement = Arrangement.spacedBy(11.dp)) {
            mounts.forEach { mount ->
                val usage = mount.gauge("usage") ?: return@forEach
                CapacityRow(
                    name = mount.display,
                    usage = usage,
                    detail = mount.gauge("used")?.let { used ->
                        mount.gauge("total")?.let { total ->
                            "${format(used)} of ${format(total)}"
                        }
                    },
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

    Panel("Temperature & power", subtitle = "${sensors.size} sensors") {
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            temperatures.forEach { sensor ->
                val temp = sensor.gauge("temp")!!
                KeyValueRow(sensor.display, format(temp), emphasis = heatColour(temp))
            }
            fans.forEach { sensor ->
                KeyValueRow(sensor.display, format(sensor.gauge("fan")!!))
            }
            power.forEach { sensor ->
                KeyValueRow(sensor.display, format(sensor.gauge("power")!!))
            }
        }
    }
}

/**
 * Warm and hot, judged against the chip's own critical point when it publishes one.
 *
 * A fixed 80°C threshold is wrong in both directions: an NVMe drive is specified to 70 and a CPU
 * package to 100, so the same number is an alarm on one and unremarkable on the other.
 */
private fun heatColour(gauge: MetricGauge): Color {
    val critical = gauge.max
    if (critical == null || critical <= 0.0) {
        return when {
            gauge.value >= 90 -> Theme.bad
            gauge.value >= 80 -> Theme.warn
            else -> Theme.primary
        }
    }
    val fraction = gauge.value / critical
    return when {
        fraction >= 0.95 -> Theme.bad
        fraction >= 0.85 -> Theme.warn
        else -> Theme.primary
    }
}

@Composable
private fun ProcessPanel(snapshot: TargetSnapshot, model: CoreModel) {
    if (snapshot.topProcesses.isEmpty()) return

    Panel("Processes", subtitle = "top ${snapshot.topProcesses.size}") {
        Column {
            snapshot.topProcesses.forEach { process ->
                Row(
                    Modifier.fillMaxWidth().padding(vertical = 5.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        process.pid,
                        color = Theme.tertiary,
                        fontSize = 11.sp,
                        fontFamily = FontFamily.Monospace,
                        modifier = Modifier.width(48.dp),
                    )
                    Text(
                        process.command,
                        color = Theme.primary,
                        fontSize = 12.5.sp,
                        maxLines = 1,
                        modifier = Modifier.weight(1f),
                    )
                    Text(
                        "%.0f%%".format(process.cpuPercent),
                        color = if (process.cpuPercent >= 50) Theme.warn else Theme.secondary,
                        fontSize = 12.sp,
                        fontFamily = FontFamily.Monospace,
                        modifier = Modifier.width(52.dp),
                    )
                    Text(
                        model.format(process.memoryBytes, "B", true),
                        color = Theme.secondary,
                        fontSize = 12.sp,
                        fontFamily = FontFamily.Monospace,
                        modifier = Modifier.width(68.dp),
                    )
                }
            }
        }
    }
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
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            // Twenty socket counters are numbers to read, not gauges to interpret.
            group.gauges.forEach { gauge ->
                KeyValueRow(
                    gauge.label,
                    format(gauge),
                    emphasis = if (gauge.metric == "tcp_retrans" && gauge.value > 0) {
                        Theme.warn
                    } else {
                        Theme.primary
                    },
                )
            }
        }
    }
}

// ---------------------------------------------------------------- pieces

@Composable
private fun Panel(
    title: String,
    subtitle: String? = null,
    content: @Composable () -> Unit,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(Theme.panel)
            .border(1.dp, Theme.border, RoundedCornerShape(14.dp))
            .padding(13.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                title,
                color = Theme.primary,
                fontSize = 12.sp,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.weight(1f),
            )
            subtitle?.let { Text(it, color = Theme.tertiary, fontSize = 10.5.sp) }
        }
        Spacer(Modifier.height(11.dp))
        content()
    }
}

@Composable
private fun KeyValueRow(label: String, value: String, emphasis: Color = Theme.primary) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(
            label,
            color = Theme.secondary,
            fontSize = 12.sp,
            maxLines = 1,
            modifier = Modifier.weight(1f),
        )
        Text(value, color = emphasis, fontSize = 12.sp, fontFamily = FontFamily.Monospace)
    }
}

@Composable
private fun CoreBar(name: String, gauge: MetricGauge, value: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(
            name,
            color = Theme.tertiary,
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
            modifier = Modifier.width(38.dp),
        )
        Box(
            Modifier
                .weight(1f)
                .height(7.dp)
                .clip(RoundedCornerShape(4.dp))
                .background(Theme.track),
        ) {
            Box(
                Modifier
                    .fillMaxWidth((gauge.fraction() ?: 0.0).toFloat())
                    .fillMaxSize()
                    .clip(RoundedCornerShape(4.dp))
                    .background(gauge.severity()),
            )
        }
        Spacer(Modifier.width(9.dp))
        Text(
            value,
            color = Theme.secondary,
            fontSize = 10.5.sp,
            fontFamily = FontFamily.Monospace,
            modifier = Modifier.width(46.dp),
        )
    }
}

@Composable
private fun CapacityRow(name: String, usage: MetricGauge, detail: String?) {
    Column {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                name,
                color = Theme.primary,
                fontSize = 12.sp,
                maxLines = 1,
                modifier = Modifier.weight(1f),
            )
            Text(
                "%.0f%%".format(usage.value),
                color = usage.severity(),
                fontSize = 12.sp,
                fontFamily = FontFamily.Monospace,
            )
        }
        Spacer(Modifier.height(5.dp))
        Box(
            Modifier
                .fillMaxWidth()
                .height(6.dp)
                .clip(RoundedCornerShape(3.dp))
                .background(Theme.track),
        ) {
            Box(
                Modifier
                    .fillMaxWidth((usage.fraction() ?: 0.0).toFloat())
                    .fillMaxSize()
                    .clip(RoundedCornerShape(3.dp))
                    .background(usage.severity()),
            )
        }
        detail?.let {
            Spacer(Modifier.height(3.dp))
            Text(it, color = Theme.tertiary, fontSize = 10.5.sp)
        }
    }
}

@Composable
private fun StatCell(
    label: String,
    value: String,
    color: Color,
    history: List<Double>,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier
            .clip(RoundedCornerShape(11.dp))
            .background(Theme.card)
            .padding(11.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(6.dp).clip(CircleShape).background(color))
            Spacer(Modifier.width(6.dp))
            Text(label, color = Theme.secondary, fontSize = 10.5.sp)
        }
        Spacer(Modifier.height(5.dp))
        Text(
            value,
            color = Theme.primary,
            fontSize = 15.sp,
            fontWeight = FontWeight.Medium,
            fontFamily = FontFamily.Monospace,
            maxLines = 1,
        )
        if (history.size > 1) {
            Spacer(Modifier.height(7.dp))
            Sparkline(history, color, Modifier.fillMaxWidth().height(20.dp))
        }
    }
}

@Composable
private fun ThroughputRow(name: String, down: String?, up: String?) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(
            name,
            color = Theme.primary,
            fontSize = 12.sp,
            maxLines = 1,
            modifier = Modifier.weight(1f),
        )
        down?.let {
            Text("↓ $it", color = Theme.secondary, fontSize = 11.sp, fontFamily = FontFamily.Monospace)
        }
        Spacer(Modifier.width(10.dp))
        up?.let {
            Text("↑ $it", color = Theme.secondary, fontSize = 11.sp, fontFamily = FontFamily.Monospace)
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

/** Colour by how close to full, for the readings where full is a problem. */
fun MetricGauge.severity(): Color {
    val fraction = fraction() ?: return Theme.info
    return when {
        fraction >= 0.9 -> Theme.bad
        fraction >= 0.75 -> Theme.warn
        else -> Theme.good
    }
}

/** "6 of 14" when a list is capped, and nothing when everything is on screen. */
private fun shown(count: Int, cap: Int): String =
    if (count > cap) "$cap of $count" else "$count"
