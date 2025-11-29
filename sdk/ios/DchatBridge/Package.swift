// swift-tools-version:5.7
// dchat iOS Native Bridge Package

import PackageDescription

let package = Package(
    name: "DchatBridge",
    platforms: [
        .iOS(.v13),
        .macOS(.v10_15)
    ],
    products: [
        .library(
            name: "DchatBridge",
            type: .static,
            targets: ["DchatBridge"]
        ),
    ],
    dependencies: [],
    targets: [
        .target(
            name: "DchatBridge",
            dependencies: [],
            path: "Sources",
            swiftSettings: [
                .define("PRODUCTION", .when(configuration: .release))
            ]
        ),
        .testTarget(
            name: "DchatBridgeTests",
            dependencies: ["DchatBridge"],
            path: "Tests"
        ),
    ]
)
