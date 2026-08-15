package cloud.lazarev.serverglass

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
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

@Composable
fun HealthCard(health: HostHealth, name: String, modifier: Modifier = Modifier) {
    val tint = Theme.health(health.level)
    Column(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(tint.copy(alpha = 0.10f))
            .border(1.dp, tint.copy(alpha = 0.30f), RoundedCornerShape(14.dp))
            .padding(14.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.size(12.dp).clip(CircleShape).background(tint),
            )
            Spacer(Modifier.width(10.dp))
            Text(
                health.headline,
                color = Theme.primary,
                fontSize = 19.sp,
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
fun SimpleTileCard(tile: SimpleTile, modifier: Modifier = Modifier) {
    Column(
        modifier
            .clip(RoundedCornerShape(12.dp))
            .background(Theme.panel)
            .border(1.dp, Theme.border, RoundedCornerShape(12.dp))
            .padding(13.dp),
    ) {
        Text(tile.name, color = Theme.secondary, fontSize = 12.sp, fontWeight = FontWeight.Medium)
        Spacer(Modifier.height(8.dp))
        Text(
            tile.valueText,
            color = Theme.primary,
            fontSize = 20.sp,
            fontWeight = FontWeight.SemiBold,
            fontFamily = FontFamily.Monospace,
        )
        tile.fraction?.let { fraction ->
            Spacer(Modifier.height(8.dp))
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(5.dp)
                    .clip(RoundedCornerShape(3.dp))
                    .background(Theme.track),
            ) {
                Box(
                    Modifier
                        .fillMaxWidth(fraction.toFloat().coerceIn(0f, 1f))
                        .height(5.dp)
                        .clip(RoundedCornerShape(3.dp))
                        .background(Theme.health(tile.level)),
                )
            }
        }
        if (tile.summary.isNotEmpty()) {
            Spacer(Modifier.height(7.dp))
            Text(tile.summary, color = Theme.tertiary, fontSize = 11.sp)
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
                // Adaptive: two columns on a folded phone, more once it opens.
                LazyVerticalGrid(
                    columns = GridCells.Adaptive(minSize = 152.dp),
                    modifier = Modifier.fillMaxWidth().height(
                        if (snapshot.simpleTiles.size > 2) 260.dp else 140.dp,
                    ),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    items(snapshot.simpleTiles.size) { index ->
                        SimpleTileCard(snapshot.simpleTiles[index])
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
