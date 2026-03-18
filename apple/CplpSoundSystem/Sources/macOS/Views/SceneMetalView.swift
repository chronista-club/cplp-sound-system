#if os(macOS)

import AppKit
import CplpBridge
import QuartzCore
import SwiftUI

// MARK: - SceneMetalView

/// wgpu Surface を表示する Metal ビュー
///
/// CAMetalLayer を作成し、cplp_scene_attach() で Rust 側の wgpu レンダラーに渡す。
/// CVDisplayLink でフレームごとに cplp_scene_render() を呼び出す。
struct SceneMetalView: View {
    @Environment(CplpClient.self) private var client

    var body: some View {
        VStack {
            if client.isInitialized {
                MetalLayerView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                Text("Runtime not initialized")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .navigationTitle("Scene")
    }
}

// MARK: - MetalLayerView (NSViewRepresentable)

struct MetalLayerView: NSViewRepresentable {

    func makeNSView(context: Context) -> MetalHostView {
        let view = MetalHostView()
        return view
    }

    func updateNSView(_ nsView: MetalHostView, context: Context) {
        // SwiftUI の再描画時にリサイズを通知
        let size = nsView.bounds.size
        let scale = nsView.window?.backingScaleFactor ?? 2.0
        let w = UInt32(size.width * scale)
        let h = UInt32(size.height * scale)
        if w > 0 && h > 0 {
            cplp_scene_resize(w, h)
        }
    }
}

// MARK: - MetalHostView

/// CAMetalLayer をホストする NSView
///
/// wantsLayer = true で CAMetalLayer を作成し、
/// CVDisplayLink で毎フレーム cplp_scene_render() を呼ぶ。
final class MetalHostView: NSView {
    private var displayLink: CVDisplayLink?
    private var isAttached = false

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        wantsLayer = true
    }

    override func makeBackingLayer() -> CALayer {
        let metalLayer = CAMetalLayer()
        metalLayer.pixelFormat = .bgra8Unorm
        metalLayer.contentsScale = NSScreen.main?.backingScaleFactor ?? 2.0
        metalLayer.framebufferOnly = true
        return metalLayer
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()

        if window != nil && !isAttached {
            attachScene()
        } else if window == nil && isAttached {
            detachScene()
        }
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        guard isAttached else { return }

        let scale = window?.backingScaleFactor ?? 2.0
        let w = UInt32(newSize.width * scale)
        let h = UInt32(newSize.height * scale)
        if w > 0 && h > 0 {
            layer?.frame = CGRect(origin: .zero, size: newSize)
            cplp_scene_resize(w, h)
        }
    }

    // MARK: - Scene Lifecycle

    private func attachScene() {
        guard let metalLayer = layer as? CAMetalLayer else { return }

        let scale = window?.backingScaleFactor ?? 2.0
        let w = UInt32(bounds.width * scale)
        let h = UInt32(bounds.height * scale)
        guard w > 0 && h > 0 else { return }

        metalLayer.contentsScale = scale
        metalLayer.drawableSize = CGSize(width: CGFloat(w), height: CGFloat(h))

        // Rust 側に CAMetalLayer を渡す
        let layerPtr = Unmanaged.passUnretained(metalLayer).toOpaque()
        let result = cplp_scene_attach(layerPtr, w, h)
        guard result == CPLP_RESULT_OK else { return }

        isAttached = true
        startDisplayLink()
    }

    private func detachScene() {
        stopDisplayLink()
        cplp_scene_detach()
        isAttached = false
    }

    // MARK: - Display Link

    private func startDisplayLink() {
        guard displayLink == nil else { return }

        var link: CVDisplayLink?
        CVDisplayLinkCreateWithActiveCGDisplays(&link)
        guard let link else { return }

        CVDisplayLinkSetOutputCallback(link, { _, _, _, _, _, _ -> CVReturn in
            cplp_scene_render()
            return kCVReturnSuccess
        }, nil)

        CVDisplayLinkStart(link)
        displayLink = link
    }

    private func stopDisplayLink() {
        guard let link = displayLink else { return }
        CVDisplayLinkStop(link)
        displayLink = nil
    }

    deinit {
        detachScene()
    }
}

#endif
