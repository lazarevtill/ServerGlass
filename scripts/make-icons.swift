#!/usr/bin/env swift
//
// Generates every app icon for every platform.
//
//   swift scripts/make-icons.swift
//
// The icon is drawn in code rather than committed as a pile of PNGs. A binary blob cannot be
// reviewed, cannot be re-rendered at a size nobody anticipated, and drifts from the app's palette
// the moment the palette changes. This reads the same colours the UI does and emits every size
// each platform asks for.
//
// The mark is the app's own signature: the ring gauge that appears on every screen, drawn open at
// the top exactly as the live gauges are, wrapped around a small server stack. At 16 points the
// stack blurs and the ring carries it, which is the right way round — the ring is what the app
// looks like.

import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

// MARK: - Palette, matching apps/shared/ServerGlassUI/DesignSystem.swift

let backgroundTop = CGColor(red: 0.105, green: 0.105, blue: 0.125, alpha: 1)
let backgroundBottom = CGColor(red: 0.035, green: 0.035, blue: 0.043, alpha: 1)
let accent = CGColor(red: 0.35, green: 0.84, blue: 0.55, alpha: 1)
let track = CGColor(red: 1, green: 1, blue: 1, alpha: 0.10)
let glyph = CGColor(red: 1, green: 1, blue: 1, alpha: 0.92)

enum Style {
    /// The complete icon, including its own rounded background.
    case full
    /// Glyph only on transparency, inset for Android's adaptive mask, which crops to roughly the
    /// middle two thirds and may show any shape from a circle to a squircle.
    case adaptiveForeground
}

func makeContext(_ size: Int) -> CGContext {
    let context = CGContext(
        data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.interpolationQuality = .high
    context.setAllowsAntialiasing(true)
    return context
}

func drawIcon(in context: CGContext, size: CGFloat, style: Style) {
    // Adaptive foregrounds are drawn smaller so the launcher's mask cannot clip the mark.
    let scale: CGFloat = style == .adaptiveForeground ? 0.58 : 1.0
    let inset = (size - size * scale) / 2

    if style == .full {
        // macOS and iOS both round the corners themselves on modern OS versions, but a rounded
        // background still matters: it is what legacy Android launchers and the .icns preview show.
        let radius = size * 0.2237  // Apple's superellipse approximation for app icons.
        let path = CGPath(
            roundedRect: CGRect(x: 0, y: 0, width: size, height: size),
            cornerWidth: radius, cornerHeight: radius, transform: nil)
        context.saveGState()
        context.addPath(path)
        context.clip()

        let gradient = CGGradient(
            colorsSpace: CGColorSpaceCreateDeviceRGB(),
            colors: [backgroundTop, backgroundBottom] as CFArray,
            locations: [0, 1])!
        context.drawLinearGradient(
            gradient, start: CGPoint(x: 0, y: size), end: CGPoint(x: size, y: 0), options: [])
        context.restoreGState()
    }

    let box = CGRect(x: inset, y: inset, width: size * scale, height: size * scale)
    let centre = CGPoint(x: box.midX, y: box.midY)
    let ringRadius = box.width * 0.335
    let ringWidth = box.width * 0.105

    // The track, then the arc over it — the same two-layer construction the live gauges use.
    context.setLineCap(.round)
    context.setLineWidth(ringWidth)

    context.setStrokeColor(track)
    context.addArc(
        center: centre, radius: ringRadius, startAngle: 0, endAngle: .pi * 2, clockwise: false)
    context.strokePath()

    // Open at the top and sweeping clockwise, matching a gauge sitting at roughly 78%.
    context.setStrokeColor(accent)
    let start = CGFloat.pi / 2 - 0.34
    context.addArc(
        center: centre, radius: ringRadius,
        startAngle: start, endAngle: start - .pi * 2 * 0.78, clockwise: true)
    context.strokePath()

    // A small server stack inside: three rounded bars, the middle one shorter so the shape is not
    // a solid block at small sizes.
    context.setFillColor(glyph)
    let barHeight = box.width * 0.052
    let gap = box.width * 0.043
    let widths: [CGFloat] = [0.30, 0.22, 0.30]
    let total = barHeight * 3 + gap * 2

    for (index, relativeWidth) in widths.enumerated() {
        let width = box.width * relativeWidth
        let y = centre.y + total / 2 - barHeight - CGFloat(index) * (barHeight + gap)
        let bar = CGRect(x: centre.x - width / 2, y: y, width: width, height: barHeight)
        context.addPath(
            CGPath(
                roundedRect: bar, cornerWidth: barHeight / 2, cornerHeight: barHeight / 2,
                transform: nil))
    }
    context.fillPath()
}

func writePNG(size: Int, style: Style, to path: String) {
    let context = makeContext(size)
    drawIcon(in: context, size: CGFloat(size), style: style)
    guard let image = context.makeImage() else { fatalError("render failed for \(path)") }

    let url = URL(fileURLWithPath: path)
    try? FileManager.default.createDirectory(
        at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    guard
        let destination = CGImageDestinationCreateWithURL(
            url as CFURL, UTType.png.identifier as CFString, 1, nil)
    else { fatalError("cannot write \(path)") }
    CGImageDestinationAddImage(destination, image, nil)
    CGImageDestinationFinalize(destination)
}

// MARK: - Output

let root = FileManager.default.currentDirectoryPath

// macOS: an .iconset that `iconutil` turns into ServerGlass.icns.
let iconset = "\(root)/target/ServerGlass.iconset"
for base in [16, 32, 128, 256, 512] {
    writePNG(size: base, style: .full, to: "\(iconset)/icon_\(base)x\(base).png")
    writePNG(size: base * 2, style: .full, to: "\(iconset)/icon_\(base)x\(base)@2x.png")
}

// iOS: modern Xcode takes a single 1024 and derives the rest.
let appicon = "\(root)/apps/ios/Assets.xcassets/AppIcon.appiconset"
writePNG(size: 1024, style: .full, to: "\(appicon)/icon-1024.png")

// Android: legacy square/round mipmaps plus an adaptive foreground per density.
let densities: [(String, Int)] = [
    ("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192),
]
let res = "\(root)/apps/android/app/src/main/res"
for (density, size) in densities {
    writePNG(size: size, style: .full, to: "\(res)/mipmap-\(density)/ic_launcher.png")
    writePNG(size: size, style: .full, to: "\(res)/mipmap-\(density)/ic_launcher_round.png")
    // Adaptive foregrounds are 108dp against a 72dp safe zone, so they are rendered larger.
    writePNG(
        size: size * 108 / 48, style: .adaptiveForeground,
        to: "\(res)/mipmap-\(density)/ic_launcher_foreground.png")
}

print("icons written")
