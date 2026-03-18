import SwiftUI
import MetalKit

/// wgpu (Rust) が CAMetalLayer に描画する 3D シーンビュー
struct SceneMetalView: NSViewRepresentable {
    func makeNSView(context: Context) -> MTKView {
        let mtkView = MTKView()
        mtkView.device = MTLCreateSystemDefaultDevice()
        mtkView.isPaused = true  // 自前の DisplayLink で駆動
        mtkView.enableSetNeedsDisplay = false
        mtkView.delegate = context.coordinator
        mtkView.layer?.isOpaque = true

        return mtkView
    }

    func updateNSView(_ nsView: MTKView, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    class Coordinator: NSObject, MTKViewDelegate {
        private var isAttached = false
        private var displayLink: CVDisplayLink?
        /// CAMetalLayer への強参照を保持（Rust 側が raw pointer で参照するため）
        private var retainedMetalLayer: CAMetalLayer?
        /// DisplayLink コールバックから安全にチェックする停止フラグ
        /// NSLock ベース（class なので ARC で管理可能）
        private let renderActive = RenderFlag()

        override init() {
            super.init()
        }

        deinit {
            stopDisplayLink()
            cplp_scene_detach()
            retainedMetalLayer = nil
        }

        // MARK: - MTKViewDelegate

        func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {
            let w = UInt32(size.width)
            let h = UInt32(size.height)

            // ゼロサイズをスキップ（初回レイアウト前に呼ばれることがある）
            guard w > 0, h > 0 else { return }

            if !isAttached {
                guard let metalLayer = view.layer as? CAMetalLayer else { return }
                retainedMetalLayer = metalLayer
                let layerPtr = Unmanaged.passUnretained(metalLayer).toOpaque()
                let result = cplp_scene_attach(layerPtr, w, h)
                if result == CPLP_RESULT_OK {
                    isAttached = true
                    startDisplayLink()
                }
            } else {
                cplp_scene_resize(w, h)
            }
        }

        func draw(in view: MTKView) {
            // DisplayLink 駆動のため未使用
        }

        // MARK: - DisplayLink

        private func startDisplayLink() {
            renderActive.isActive = true

            var link: CVDisplayLink?
            CVDisplayLinkCreateWithActiveCGDisplays(&link)
            guard let link else { return }

            // RenderFlag を passRetained でコールバックに渡す
            let flagPtr = Unmanaged.passRetained(renderActive).toOpaque()

            CVDisplayLinkSetOutputCallback(link, { _, _, _, _, _, userInfo -> CVReturn in
                guard let userInfo else { return kCVReturnSuccess }
                let flag = Unmanaged<RenderFlag>.fromOpaque(userInfo).takeUnretainedValue()
                if flag.isActive {
                    cplp_scene_render()
                }
                return kCVReturnSuccess
            }, flagPtr)

            CVDisplayLinkStart(link)
            self.displayLink = link
        }

        private func stopDisplayLink() {
            // フラグを先に無効化（コールバックが render を呼ばなくなる）
            renderActive.isActive = false

            if let link = displayLink {
                CVDisplayLinkStop(link)
                displayLink = nil
            }

            // passRetained の対の release（DisplayLink 停止後に安全に解放）
            if isAttached {
                Unmanaged.passUnretained(renderActive).release()
            }
        }
    }
}

/// DisplayLink コールバック（別スレッド）から安全にアクセスできるフラグ
/// class 型なので Unmanaged/ARC で管理可能
private final class RenderFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var _isActive: Bool = false

    var isActive: Bool {
        get { lock.withLock { _isActive } }
        set { lock.withLock { _isActive = newValue } }
    }
}
