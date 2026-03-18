// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "CplpSoundSystem",
    platforms: [
        .macOS(.v26)
    ],
    products: [
        .executable(name: "CplpSoundSystem", targets: ["CplpSoundSystem"])
    ],
    targets: [
        .executableTarget(
            name: "CplpSoundSystem",
            dependencies: ["CplpBridge"],
            path: "Sources/macOS",
            linkerSettings: [
                // libcplp_ffi.a をリンク（cargo build --release で生成）
                .unsafeFlags([
                    "-L../../target/release",
                    "-lcplp_ffi",
                ]),
            ]
        ),
        // cplp-ffi の C ヘッダーを Swift から使えるようにする
        .systemLibrary(
            name: "CplpBridge",
            path: "CplpBridge"
        )
    ]
)
