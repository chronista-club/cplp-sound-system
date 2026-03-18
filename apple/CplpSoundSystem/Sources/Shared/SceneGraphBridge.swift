#if os(visionOS)

import RealityKit
import SwiftUI

// MARK: - SceneGraphBridge

/// FFI の SceneGraph データを Swift の RealityKit Entity に変換するブリッジ
///
/// Rust 側の SceneGraph / SceneNode 構造体を Swift の SceneNodeData として受け取り、
/// RealityKit の Entity 階層に変換する。
///
/// ## 変換ルール
///
/// | Rust (SceneNode)      | RealityKit (Entity)            |
/// |-----------------------|--------------------------------|
/// | name                  | Entity.name                    |
/// | transform.position    | Entity.position                |
/// | transform.scale       | Entity.scale                   |
/// | transform.rotation    | Entity.orientation (Euler→Quat)|
/// | mesh (Some)           | ModelEntity + box mesh         |
/// | mesh (None)           | Entity (group node)            |
/// | animation             | TODO: RealityKit animation     |
/// | children              | Entity.addChild() 再帰         |
///
/// ## SceneGraph → Entity 変換フロー
///
/// ```
/// FFI: cplp_scene_get_nodes()
///   ↓ C 構造体配列
/// Swift: SceneNodeData (値型に変換)
///   ↓ SceneGraphBridge.createEntity()
/// RealityKit: Entity 階層
///   ↓ RealityView content.add()
/// visionOS: 空間レンダリング
/// ```
enum SceneGraphBridge {

    // MARK: - Entity Creation

    /// SceneNodeData から RealityKit Entity を生成
    ///
    /// メッシュデータがある場合は ModelEntity（ボックスメッシュ）を生成し、
    /// ない場合はグループ Entity を生成する。子ノードは再帰的に変換される。
    static func createEntity(from node: SceneNodeData) -> Entity {
        let entity: Entity

        // メッシュノードかグループノードかで分岐
        // SceneGraph のメッシュデータは任意の頂点データだが、
        // visionOS では簡略化してボックスメッシュで表現する
        let material = SimpleMaterial(
            color: uiColor(from: node.color),
            isMetallic: false
        )
        let mesh = MeshResource.generateBox(
            width: node.scale.x,
            height: node.scale.y,
            depth: max(node.scale.z, 0.01)  // 最小奥行きを確保
        )
        let modelEntity = ModelEntity(mesh: mesh, materials: [material])
        modelEntity.name = node.id
        modelEntity.position = node.position

        entity = modelEntity

        // 子ノードを再帰的に変換
        for child in node.children {
            let childEntity = createEntity(from: child)
            entity.addChild(childEntity)
        }

        return entity
    }

    // MARK: - Batch Update

    /// SceneGraph の全ノードから Entity 階層を一括生成
    ///
    /// ルート Entity を返す。RealityView の content.add() に渡す。
    static func createRootEntity(
        from nodes: [SceneNodeData],
        rackConfig: RackConfigData = .default
    ) -> Entity {
        let root = Entity()
        root.name = "SceneGraphRoot"

        // ラックフレームの背景パネル
        let backdrop = createBackdrop(rackConfig: rackConfig)
        root.addChild(backdrop)

        // 各ノードを Entity に変換して追加
        for node in nodes {
            let entity = createEntity(from: node)
            root.addChild(entity)
        }

        return root
    }

    /// Entity 階層を SceneNodeData で差分更新
    ///
    /// 既存の Entity を名前で検索し、位置・スケール・色を更新する。
    /// 新規ノードは追加、不在ノードは削除する。
    static func updateEntities(
        root: Entity,
        with nodes: [SceneNodeData]
    ) {
        let existingNames = Set(root.children.map(\.name))
        let newNames = Set(nodes.map(\.id))

        // 削除されたノードを除去
        for name in existingNames.subtracting(newNames) {
            if let entity = root.findEntity(named: name) {
                entity.removeFromParent()
            }
        }

        // 更新・追加
        for node in nodes {
            if let existing = root.findEntity(named: node.id) {
                // 位置・スケール更新
                existing.position = node.position
                existing.scale = node.scale

                // 色更新（ModelEntity の場合）
                if let model = existing as? ModelEntity {
                    let material = SimpleMaterial(
                        color: uiColor(from: node.color),
                        isMetallic: false
                    )
                    model.model?.materials = [material]
                }
            } else {
                // 新規追加
                let entity = createEntity(from: node)
                root.addChild(entity)
            }
        }
    }

    // MARK: - Helpers

    /// ラック背景パネルを作成
    private static func createBackdrop(rackConfig: RackConfigData) -> Entity {
        let hpWidth: Float = 0.00508
        let moduleHeight: Float = 0.1286
        let width = Float(rackConfig.totalHP) * hpWidth
        let height = Float(rackConfig.rows) * moduleHeight

        let color = uiColor(from: rackConfig.frameColor).withAlphaComponent(0.15)
        let material = SimpleMaterial(color: color, isMetallic: false)
        let backdrop = ModelEntity(
            mesh: .generateBox(size: [width, height, 0.002]),
            materials: [material]
        )
        backdrop.name = "Backdrop"
        backdrop.position = SIMD3(width / 2, -height / 2, -0.01)

        return backdrop
    }

    /// SIMD3<Float> [r, g, b] から UIColor に変換
    private static func uiColor(from rgb: SIMD3<Float>) -> UIColor {
        UIColor(
            red: CGFloat(rgb.x),
            green: CGFloat(rgb.y),
            blue: CGFloat(rgb.z),
            alpha: 1.0
        )
    }
}

// MARK: - RackConfigData

/// RackConfig の Swift 表現
///
/// Rust 側の RackConfig 構造体に対応する。
struct RackConfigData {
    let totalHP: Int
    let rows: Int
    let frameColor: SIMD3<Float>

    static let `default` = RackConfigData(
        totalHP: 84,
        rows: 2,
        frameColor: SIMD3(0.42, 0.42, 0.46)
    )
}

// MARK: - FFI Data Conversion

/// FFI C 構造体から SceneNodeData への変換ヘルパー
///
/// cplp_scene_get_nodes() が実装されたら、この関数で C 構造体配列を
/// Swift の値型に変換する。
///
/// ```
/// // 将来の使用例:
/// let cNodes = cplp_scene_get_nodes()
/// let swiftNodes = SceneGraphBridge.convertFFINodes(cNodes)
/// let root = SceneGraphBridge.createRootEntity(from: swiftNodes)
/// ```
extension SceneGraphBridge {

    /// FFI Transform → SIMD3 position
    static func position(from transform: (Float, Float, Float)) -> SIMD3<Float> {
        SIMD3(transform.0, transform.1, transform.2)
    }

    /// FFI Transform → SIMD3 scale
    static func scale(from transform: (Float, Float, Float)) -> SIMD3<Float> {
        SIMD3(transform.0, transform.1, transform.2)
    }

    /// FFI color [r, g, b] → SIMD3<Float>
    static func color(from rgb: (Float, Float, Float)) -> SIMD3<Float> {
        SIMD3(rgb.0, rgb.1, rgb.2)
    }

    // TODO: FFI SceneNode 構造体が cbindgen で生成されたら、以下を実装:
    //
    // static func convertFFINodes(_ ptr: UnsafePointer<CplpSceneNode>, count: Int) -> [SceneNodeData] {
    //     (0..<count).map { i in
    //         let cNode = ptr[i]
    //         return SceneNodeData(
    //             id: String(cString: cNode.name),
    //             name: String(cString: cNode.name),
    //             position: position(from: (cNode.position.0, cNode.position.1, cNode.position.2)),
    //             scale: scale(from: (cNode.scale.0, cNode.scale.1, cNode.scale.2)),
    //             color: color(from: (cNode.color.0, cNode.color.1, cNode.color.2)),
    //             children: []  // TODO: 再帰的に変換
    //         )
    //     }
    // }
}

// MARK: - PHASE Spatial Audio Integration Notes

// ── PHASE フレームワークによる空間オーディオ統合メモ ──────────────────
//
// visionOS での空間オーディオ統合に向けた設計メモ。
// 現時点では視覚的な空間配置のみ実装し、オーディオ統合は Phase 4 で対応。
//
// 1. PHASESpatialMixer による統合案:
//    - PHASEEngine を初期化し、各トラックに PHASESource を作成
//    - SceneGraphBridge で Entity 位置が変更された際に、
//      対応する PHASESource の transform も同期させる
//    - PHASEListener をユーザーの頭部位置（ARKit WorldTrackingProvider）に同期
//
// 2. RealityKit SpatialAudioComponent による統合案（visionOS 2.0+）:
//    - 各トラックの Entity に SpatialAudioComponent を追加
//    - AudioFileResource でストリーミング再生
//    - RealityKit が自動的に HRTF + 距離減衰を適用
//    - より簡単だが、カスタムオーディオストリーム（cpal 経由）との統合が課題
//
// 3. Rust cpal → PHASE ブリッジ案:
//    - cpal の出力を AudioUnit 経由で PHASE に接続
//    - AURenderCallback で Rust のオーディオバッファを PHASE に流し込む
//    - レイテンシの考慮が必要（Phase 4 で詳細設計）
//
// 4. 空間配置とオーディオの同期:
//    - SpatialMixerView の track.spatialPosition が更新されたら
//      対応する PHASESource/SpatialAudioComponent の位置も更新
//    - ドラッグ操作中はリアルタイムで音像が移動する体験を目指す

#endif
