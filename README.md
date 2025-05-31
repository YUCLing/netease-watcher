# Netease Watcher
实时监听网易云音乐当前播放歌曲和播放进度，理论兼容`3.x`所有版本

## 使用方法

1. 启动程序
2. 使用 HTTP 或 WebSocket 连接以获取当前进度和歌曲信息

### HTTP

除 `/ws` 以外的任何地址均可获取当前进度和歌曲信息。

#### 返回示例

```json
{
    "music": {
        "album": "离岛之歌",
        "aliases": [
            "手游《阴阳师》SSR不知火式神主题曲"
        ],
        "artists": [
            "東山奈央"
        ],
        "duration": 226046,
        "id": 1359559416,
        "name": "离岛之歌",
        "thumbnail": "http://p3.music.126.net/u_7WmtvEGYB-C3t4wmCtYA==/109951164007467060.jpg"
    },
    "time": 41.535
}
```

### WebSocket

使用地址 `/ws` 发起 WebSocket 连接，连接成功后会直接发送当前进度和歌曲信息

在当前进度或歌曲发生变化时会发送新的 JSON 数据

#### 歌曲信息示例

```json
{
    "type": "musicchange",
    "value": {
        "album": "离音",
        "aliases": [
            "手游《阴阳师》SSR不知火式神主题曲中文版"
        ],
        "artists": [
            "林孟璇"
        ],
        "duration": 256800,
        "id": 1361747616,
        "name": "离音",
        "thumbnail": "http://p4.music.126.net/xqA_38tqlW8f_JUYnCGAVQ==/109951164017543788.jpg"
    }
}
```

#### 播放进度示例

```json
{
    "type": "timechange",
    "value": 169.837
}
```

## 常见问题

### 如何修改监听地址

指定环境变量：
- `HOST`: 监听地址（默认`127.0.0.1`）
- `PORT`: 监听端口（默认`3574`）

### 网易云音乐主窗口会未响应
由于网易云音乐最小化一段时间后会导致数据库停止更新，故该程序会修改网易云音乐最小化行为，会偶发此BUG，目前还未修复

## 编译

1. 安装Rust
2. 运行`cargo build`

注：如不需要文字UI，则运行`cargo build --no-default-features`