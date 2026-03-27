# Geo Tool

`geo` 工具用于读取当前环境通过系统定位服务提供的坐标信息。

## 目标

- 给模型一个统一的“当前位置”读取入口。
- 直接复用 macOS `CoreLocation`，而不是退化成 IP 地理定位。
- 在权限被拒、定位服务关闭或超时时返回清晰错误。

## 代码位置

- 工具实现：`klaw-tool/src/geo.rs`
- 工具导出：`klaw-tool/src/lib.rs`
- 运行时注册：`klaw-cli/src/runtime/mod.rs`
- 配置开关：`klaw-config/src/lib.rs`

## 使用方式

`geo` 当前不需要参数：

```json
{}
```

调用成功后会返回：

- `latitude`
- `longitude`
- `horizontal_accuracy_meters`
- `vertical_accuracy_meters`
- `altitude_meters`
- `timestamp_unix_seconds`
- `source`
- `accuracy_authorization`

## 平台与权限

- 当前仅支持 macOS。
- 首次调用时，系统可能弹出定位授权提示。
- 打包后的 macOS App 需要在 `Info.plist` 中声明 `NSLocationWhenInUseUsageDescription`。

如果出现以下情况，工具会返回错误而不是静默失败：

- 系统定位服务已关闭
- 用户拒绝了定位权限
- 定位请求在超时窗口内没有返回坐标

## 配置

```toml
[tools.geo]
enabled = false
```

默认值为 `false`。
