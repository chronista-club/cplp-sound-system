#if os(visionOS)

import SwiftUI
import RealityKit

// MARK: - SpatialMixerView

/// 空間 UI でミキサーフェーダーを 3D 配置するビュー
///
/// 各ピアのオーディオソースを空間上に配置し、
/// ドラッグによるボリューム/パン操作と位置ベースの空間オーディオを実現する。
///
/// ## 空間配置ルール
/// - X 軸: パン（左 -1.0 〜 右 1.0）
/// - Y 軸: ボリューム（下 0.0 〜 上 1.0）
/// - Z 軸: 奥行き（将来の空間オーディオ距離減衰用）
///
/// ## PHASE 空間オーディオ（調査メモ）
/// - PHASESpatialMixer で各トラックの AudioSource を 3D 配置可能
/// - PHASEListener の位置をカメラ（ユーザー頭部）に同期させる
/// - RealityKit の SpatialAudioComponent も visionOS 2.0+ で利用可能
/// - 現時点では視覚的な空間配置のみ実装し、オーディオ統合は Phase 4 で対応
struct SpatialMixerView: View {
    @Environment(CplpClient.self) private var client
    @State private var selectedTrackId: String?

    var body: some View {
        ZStack {
            // 3D ミキサー空間
            RealityView { content in
                let root = Entity()
                root.name = "MixerRoot"
                root.position = SIMD3(0, 0, 0)

                // グリッドガイドを作成
                let grid = createMixerGrid()
                root.addChild(grid)

                // トラックエンティティを配置
                client.refreshMixerTracks()
                for track in client.mixerTracks {
                    let entity = createTrackEntity(track)
                    root.addChild(entity)
                }

                content.add(root)
            } update: { content in
                // トラック更新を反映
            }
            // フェーダードラッグ
            .gesture(
                DragGesture()
                    .targetedToAnyEntity()
                    .onChanged { value in
                        handleFaderDrag(value.entity, translation: value.translation3D)
                    }
                    .onEnded { value in
                        handleFaderDragEnd(value.entity)
                    }
            )
            // トラック選択
            .gesture(
                SpatialTapGesture()
                    .targetedToAnyEntity()
                    .onEnded { value in
                        selectedTrackId = value.entity.name
                    }
            )

            // オーバーレイ UI（選択中のトラック情報）
            VStack {
                Spacer()
                if let trackId = selectedTrackId,
                   let track = client.mixerTracks.first(where: { $0.peerId == trackId }) {
                    TrackInfoPanel(track: track)
                        .padding()
                        .glassBackgroundEffect()
                }
            }
        }
        .onAppear {
            client.refreshMixerTracks()
        }
    }

    // MARK: - Grid

    /// ミキサー空間のグリッドガイド
    ///
    /// パン（L/R）とボリューム（0-100%）の軸を視覚化する。
    private func createMixerGrid() -> Entity {
        let grid = Entity()
        grid.name = "MixerGrid"

        let lineColor = UIColor(white: 0.4, alpha: 0.3)
        let lineMaterial = SimpleMaterial(color: lineColor, isMetallic: false)

        // X 軸ガイド（パン: L-C-R）
        let panLine = ModelEntity(
            mesh: .generateBox(size: [1.0, 0.001, 0.001]),
            materials: [lineMaterial]
        )
        panLine.position = SIMD3(0, 0, -0.5)
        grid.addChild(panLine)

        // Y 軸ガイド（ボリューム: 0-100%）
        let volLine = ModelEntity(
            mesh: .generateBox(size: [0.001, 0.3, 0.001]),
            materials: [lineMaterial]
        )
        volLine.position = SIMD3(0, 0.15, -0.5)
        grid.addChild(volLine)

        // センターマーク
        let center = ModelEntity(
            mesh: .generateSphere(radius: 0.005),
            materials: [SimpleMaterial(color: .white.withAlphaComponent(0.5), isMetallic: false)]
        )
        center.position = SIMD3(0, 0.15, -0.5)
        grid.addChild(center)

        return grid
    }

    // MARK: - Track Entities

    /// ミキサートラックの 3D エンティティを作成
    ///
    /// 球体 + ラベルでピアを表現し、位置がボリューム/パンに対応する。
    private func createTrackEntity(_ track: MixerTrackData) -> Entity {
        let entity = Entity()
        entity.name = track.peerId

        // トラック球体
        let color: UIColor = track.isMuted
            ? UIColor(white: 0.3, alpha: 0.5)
            : trackColor(for: track.peerId)

        let sphere = ModelEntity(
            mesh: .generateSphere(radius: 0.025),
            materials: [SimpleMaterial(color: color, isMetallic: false)]
        )

        // ジェスチャー対応
        sphere.components.set(InputTargetComponent())
        sphere.components.set(
            CollisionComponent(shapes: [.generateSphere(radius: 0.03)])
        )
        sphere.components.set(HoverEffectComponent())

        entity.addChild(sphere)
        entity.position = track.spatialPosition

        // ソロ表示: 光るリング
        if track.isSolo {
            let ring = ModelEntity(
                mesh: .generateSphere(radius: 0.032),
                materials: [SimpleMaterial(
                    color: UIColor.yellow.withAlphaComponent(0.3),
                    isMetallic: true
                )]
            )
            entity.addChild(ring)
        }

        return entity
    }

    /// ピア ID に基づく色分け
    private func trackColor(for peerId: String) -> UIColor {
        let hash = peerId.hashValue
        let hue = CGFloat(abs(hash) % 360) / 360.0
        return UIColor(hue: hue, saturation: 0.7, brightness: 0.9, alpha: 1.0)
    }

    // MARK: - Fader Interaction

    /// フェーダードラッグ: X = パン, Y = ボリューム
    private func handleFaderDrag(_ entity: Entity, translation: SIMD3<Float>) {
        let sensitivity: Float = 0.002

        // パン更新（X 軸）
        let newX = entity.position.x + translation.x * sensitivity
        entity.position.x = max(-0.5, min(0.5, newX))

        // ボリューム更新（Y 軸）
        let newY = entity.position.y + translation.y * sensitivity
        entity.position.y = max(0, min(0.3, newY))
    }

    /// フェーダードラッグ終了: FFI 経由でミキサー値を更新
    private func handleFaderDragEnd(_ entity: Entity) {
        let peerId = entity.name

        // 位置からパン/ボリュームに逆変換
        let pan = entity.position.x / 0.5  // -1.0 〜 1.0
        let volume = entity.position.y / 0.3  // 0.0 〜 1.0

        // FFI 呼び出し
        peerId.withCString { cStr in
            cplp_mixer_set_volume(cStr, volume)
            cplp_mixer_set_pan(cStr, pan)
        }
    }
}

// MARK: - Track Info Panel

/// 選択中トラックの情報パネル
struct TrackInfoPanel: View {
    let track: MixerTrackData

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(track.label)
                .font(.headline)

            HStack {
                Label("Vol", systemImage: "speaker.wave.2")
                Text(String(format: "%.0f%%", track.volume * 100))
                    .monospacedDigit()
            }

            HStack {
                Label("Pan", systemImage: "arrow.left.and.right")
                Text(panLabel(track.pan))
                    .monospacedDigit()
            }

            HStack(spacing: 16) {
                Label(
                    track.isMuted ? "Muted" : "Active",
                    systemImage: track.isMuted ? "speaker.slash" : "speaker"
                )
                .foregroundStyle(track.isMuted ? .red : .primary)

                if track.isSolo {
                    Label("Solo", systemImage: "star.fill")
                        .foregroundStyle(.yellow)
                }
            }
        }
        .padding()
        .frame(width: 200)
    }

    private func panLabel(_ pan: Float) -> String {
        if abs(pan) < 0.05 { return "C" }
        let pct = Int(abs(pan) * 100)
        return pan < 0 ? "L\(pct)" : "R\(pct)"
    }
}

// MARK: - Preview

#Preview {
    SpatialMixerView()
        .environment(CplpClient())
}

#endif
