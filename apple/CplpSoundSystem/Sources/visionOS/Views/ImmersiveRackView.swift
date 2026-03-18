#if os(visionOS)

import SwiftUI
import RealityKit

// MARK: - ImmersiveRackView

/// イマーシブ空間でユーロラックを 3D 表示する
///
/// SceneGraph のノード情報をもとに RealityKit Entity を配置する。
/// 各モジュールはボックスメッシュで表現され、ハンドジェスチャーで操作可能。
struct ImmersiveRackView: View {
    @Environment(CplpClient.self) private var client
    @State private var selectedModuleId: String?
    @State private var rackEntity = Entity()

    /// ラックフレームの配置オフセット（ユーザーの前方 1m）
    private let rackOffset = SIMD3<Float>(0, 1.2, -1.0)

    var body: some View {
        RealityView { content in
            // ルートエンティティをセットアップ
            let root = Entity()
            root.name = "RackRoot"
            root.position = rackOffset

            // ラックフレームを作成
            let frame = createRackFrame()
            root.addChild(frame)

            // SceneGraph からモジュールを配置
            client.refreshSceneGraph()
            for node in client.sceneNodes {
                let entity = SceneGraphBridge.createEntity(from: node)
                // InputTargetComponent と CollisionComponent を追加（ジェスチャー用）
                if let model = entity as? ModelEntity {
                    model.components.set(InputTargetComponent())
                    let bounds = model.visualBounds(relativeTo: nil)
                    model.components.set(
                        CollisionComponent(shapes: [.generateBox(size: bounds.extents)])
                    )
                }
                root.addChild(entity)
            }

            rackEntity = root
            content.add(root)
        } update: { content in
            // SceneGraph 更新時にエンティティを再配置
            updateModuleEntities()
        }
        // タップジェスチャー: モジュール選択
        .gesture(
            SpatialTapGesture()
                .targetedToAnyEntity()
                .onEnded { value in
                    let tappedEntity = value.entity
                    handleModuleSelection(tappedEntity)
                }
        )
        // ドラッグジェスチャー: モジュール移動
        .gesture(
            DragGesture()
                .targetedToAnyEntity()
                .onChanged { value in
                    handleModuleDrag(value.entity, translation: value.translation3D)
                }
                .onEnded { value in
                    handleModuleDragEnd(value.entity)
                }
        )
        .onAppear {
            client.refreshSceneGraph()
        }
    }

    // MARK: - Rack Frame

    /// ラックフレーム（外枠）を作成
    ///
    /// RackConfig のパラメータに基づいてフレームサイズを決定。
    /// 84HP x 2 rows がデフォルト。
    private func createRackFrame() -> Entity {
        let hpWidth: Float = 0.00508  // 1HP = 5.08mm
        let moduleHeight: Float = 0.1286  // 3U = 128.6mm
        let totalHP: Int = 84
        let rows: Int = 2

        let rackWidth = Float(totalHP) * hpWidth
        let rackHeight = Float(rows) * moduleHeight + Float(rows - 1) * 0.015
        let frameDepth: Float = 0.05
        let frameThickness: Float = 0.008

        let frameEntity = Entity()
        frameEntity.name = "RackFrame"

        // フレームカラー（RackConfig.frame_color に対応）
        let frameColor: UIColor = UIColor(
            red: CGFloat(0.42),
            green: CGFloat(0.42),
            blue: CGFloat(0.46),
            alpha: 1.0
        )
        let frameMaterial = SimpleMaterial(color: frameColor, isMetallic: true)

        // 上辺
        let topBar = ModelEntity(
            mesh: .generateBox(size: [rackWidth + frameThickness * 2, frameThickness, frameDepth]),
            materials: [frameMaterial]
        )
        topBar.position = SIMD3(rackWidth / 2, frameThickness / 2, 0)
        frameEntity.addChild(topBar)

        // 下辺
        let bottomBar = ModelEntity(
            mesh: .generateBox(size: [rackWidth + frameThickness * 2, frameThickness, frameDepth]),
            materials: [frameMaterial]
        )
        bottomBar.position = SIMD3(rackWidth / 2, -(rackHeight + frameThickness / 2), 0)
        frameEntity.addChild(bottomBar)

        // 左辺
        let leftBar = ModelEntity(
            mesh: .generateBox(size: [frameThickness, rackHeight + frameThickness * 2, frameDepth]),
            materials: [frameMaterial]
        )
        leftBar.position = SIMD3(-frameThickness / 2, -rackHeight / 2, 0)
        frameEntity.addChild(leftBar)

        // 右辺
        let rightBar = ModelEntity(
            mesh: .generateBox(size: [frameThickness, rackHeight + frameThickness * 2, frameDepth]),
            materials: [frameMaterial]
        )
        rightBar.position = SIMD3(rackWidth + frameThickness / 2, -rackHeight / 2, 0)
        frameEntity.addChild(rightBar)

        // 中間レール（行の境界）
        if rows > 1 {
            for row in 1..<rows {
                let railY = -Float(row) * (moduleHeight + 0.015) + 0.015 / 2
                let rail = ModelEntity(
                    mesh: .generateBox(size: [rackWidth, 0.004, frameDepth]),
                    materials: [frameMaterial]
                )
                rail.position = SIMD3(rackWidth / 2, railY, 0)
                frameEntity.addChild(rail)
            }
        }

        return frameEntity
    }

    // MARK: - Module Interaction

    /// モジュール選択処理
    private func handleModuleSelection(_ entity: Entity) {
        // 前回の選択をリセット
        if let prevId = selectedModuleId,
           let prevEntity = rackEntity.findEntity(named: prevId) as? ModelEntity {
            // 選択ハイライトを除去
            prevEntity.components.remove(HoverEffectComponent.self)
        }

        selectedModuleId = entity.name

        // 選択ハイライト
        if let modelEntity = entity as? ModelEntity {
            modelEntity.components.set(HoverEffectComponent())
        }
    }

    /// モジュールドラッグ処理（ラック内での並び替え）
    private func handleModuleDrag(_ entity: Entity, translation: SIMD3<Float>) {
        // X 軸方向のみ移動を許可（ラック内のスロット移動）
        let clampedX = entity.position.x + translation.x * 0.001
        entity.position.x = clampedX
    }

    /// モジュールドラッグ終了（スナップ処理）
    private func handleModuleDragEnd(_ entity: Entity) {
        // HP グリッドにスナップ
        let hpWidth: Float = 0.00508
        let snappedX = round(entity.position.x / hpWidth) * hpWidth
        entity.position.x = snappedX

        // TODO: FFI 経由で SceneGraph のノード位置を更新
    }

    /// SceneGraph 更新時のエンティティ再配置
    private func updateModuleEntities() {
        for node in client.sceneNodes {
            if let entity = rackEntity.findEntity(named: node.id) {
                entity.position = node.position
                entity.scale = node.scale
            }
        }
    }
}

// MARK: - Preview

#Preview(immersionStyle: .mixed) {
    ImmersiveRackView()
        .environment(CplpClient())
}

#endif
