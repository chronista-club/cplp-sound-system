import SwiftUI

// MARK: - App Entry Point

@main
struct CplpSoundSystemApp: App {
    @State private var client = CplpClient()

    var body: some Scene {
        WindowGroup("CPLP Sound System") {
            ContentView()
                .environment(client)
        }
        .defaultSize(width: 900, height: 650)
    }
}
