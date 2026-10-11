// Regenerate with: swift -module-cache-path target/swift-cache scripts/generate-icons.swift
import AppKit
import Foundation

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
func line(_ points: [NSPoint], width: CGFloat, color: NSColor) {
    let path = NSBezierPath()
    path.lineWidth = width
    path.lineCapStyle = .round
    path.lineJoinStyle = .round
    path.move(to: points[0])
    for point in points.dropFirst() { path.line(to: point) }
    color.setStroke()
    path.stroke()
}
func mark(_ color: NSColor) {
    // Three audio bars flow into two independent output paths.
    for (x, low, high) in [(24.0, 43.0, 57.0), (34.0, 32.0, 68.0), (44.0, 39.0, 61.0)] {
        line([NSPoint(x: x, y: low), NSPoint(x: x, y: high)], width: 6, color: color)
    }
    line([NSPoint(x: 44, y: 50), NSPoint(x: 57, y: 50), NSPoint(x: 69, y: 68), NSPoint(x: 78, y: 68)], width: 5, color: color)
    line([NSPoint(x: 57, y: 50), NSPoint(x: 69, y: 32), NSPoint(x: 78, y: 32)], width: 5, color: color)
    color.setFill()
    for y in [32.0, 68.0] { NSBezierPath(ovalIn: NSRect(x: 73, y: y - 5, width: 10, height: 10)).fill() }
}
func render(size: Int, template: Bool, to url: URL) throws {
    let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    let context = NSGraphicsContext(bitmapImageRep: bitmap)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    let transform = NSAffineTransform()
    transform.scale(by: CGFloat(size) / 100)
    transform.concat()
    if !template {
        let shape = NSBezierPath(roundedRect: NSRect(x: 6, y: 6, width: 88, height: 88), xRadius: 21, yRadius: 21)
        let gradient = NSGradient(starting: NSColor(srgbRed: 0.12, green: 0.29, blue: 0.86, alpha: 1), ending: NSColor(srgbRed: 0.10, green: 0.73, blue: 0.78, alpha: 1))!
        gradient.draw(in: shape, angle: 65)
        NSColor.white.withAlphaComponent(0.18).setStroke()
        shape.lineWidth = 0.7
        shape.stroke()
    } else {
        // Use more of the tiny menu-bar canvas.
        let fit = NSAffineTransform()
        fit.translateX(by: -23, yBy: -15)
        fit.scale(by: 1.45)
        fit.concat()
    }
    mark(template ? .black : .white)
    NSGraphicsContext.restoreGraphicsState()
    try bitmap.representation(using: .png, properties: [:])!.write(to: url)
}
let icons = root.appendingPathComponent("target/AppIcon.iconset")
try FileManager.default.createDirectory(at: icons, withIntermediateDirectories: true)
for size in [16, 32, 128, 256, 512] {
    try render(size: size, template: false, to: icons.appendingPathComponent("icon_\(size)x\(size).png"))
    try render(size: size * 2, template: false, to: icons.appendingPathComponent("icon_\(size)x\(size)@2x.png"))
}
try render(size: 512, template: false, to: root.appendingPathComponent("docs/images/icon.png"))
try render(size: 36, template: true, to: root.appendingPathComponent("resources/MenuBarTemplate.png"))
