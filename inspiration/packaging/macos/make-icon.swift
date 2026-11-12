// Build-time icon asset only. The application itself is Rust/Iced.
import AppKit
import Foundation

guard CommandLine.arguments.count == 2 else { fatalError("Usage: make-icon.swift NEW_ICONSET_DIRECTORY") }
let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
guard !FileManager.default.fileExists(atPath: directory.path) else { fatalError("Refusing to overwrite existing iconset") }
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
let entries: [(String, Int)] = [("icon_16x16.png",16),("icon_16x16@2x.png",32),("icon_32x32.png",32),("icon_32x32@2x.png",64),("icon_128x128.png",128),("icon_128x128@2x.png",256),("icon_256x256.png",256),("icon_256x256@2x.png",512),("icon_512x512.png",512),("icon_512x512@2x.png",1024)]
for (name, pixels) in entries {
    guard let bitmap = NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:pixels,pixelsHigh:pixels,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0), let context = NSGraphicsContext(bitmapImageRep:bitmap) else { fatalError("Cannot allocate icon bitmap") }
    NSGraphicsContext.saveGraphicsState(); NSGraphicsContext.current = context
    let scale = CGFloat(pixels) / 1024
    context.cgContext.scaleBy(x:scale,y:scale)
    NSColor(calibratedRed:0.94,green:0.91,blue:0.84,alpha:1).setFill()
    NSBezierPath(roundedRect:NSRect(x:24,y:24,width:976,height:976),xRadius:205,yRadius:205).fill()
    NSColor(calibratedRed:0.76,green:0.48,blue:0.16,alpha:1).setFill()
    NSBezierPath(roundedRect:NSRect(x:100,y:94,width:824,height:16),xRadius:8,yRadius:8).fill()
    let paragraph = NSMutableParagraphStyle(); paragraph.alignment = .center
    let font = NSFont(name:"PingFangSC-Semibold",size:650) ?? NSFont.systemFont(ofSize:650,weight:.semibold)
    let text = NSAttributedString(string:"炉",attributes:[.font:font,.foregroundColor:NSColor(calibratedRed:0.25,green:0.21,blue:0.16,alpha:1),.paragraphStyle:paragraph])
    text.draw(in:NSRect(x:92,y:138,width:840,height:760))
    NSGraphicsContext.restoreGraphicsState()
    guard let data = bitmap.representation(using:.png,properties:[:]) else { fatalError("Cannot encode icon PNG") }
    try data.write(to:directory.appendingPathComponent(name),options:.withoutOverwriting)
}
