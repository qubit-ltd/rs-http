# qubit-http 设计文档

[English Design](design.md) | [用户指南](user_guide.zh_CN.md) | [README](../README.zh_CN.md)

本文记录 `qubit-http` 0.16.0 的执行边界和必须保持的约束。具体行为以源码和测试为准；这里重点说明调用方和维护者容易误解的设计选择。

## 组件与所有权

```mermaid
flowchart LR
    Options[HttpClientOptions] --> Builder[HttpClientBuilder]
    Builder --> Client[HttpClient]
    Client --> Request[HttpRequestBuilder / HttpRequest]
    Request --> Execute[execute / retry attempts]
    Execute --> Response[HttpResponse]
    Response --> Body[bytes / text / json / stream]
    Response --> SSE[SSE decoders]
    Client --> Reconnect[SseReconnectRunner]
    Reconnect --> Execute
    Reconnect --> SSE
```

`HttpClientBuilder` 校验 `HttpClientOptions`，再构造底层 `reqwest::Client`。客户端持有连接池、默认策略、注入器、拦截器和脱敏策略快照。`HttpRequestBuilder` 将请求级设置与客户端默认值收集到 `HttpRequest`；发送请求时无需借用可变的客户端配置。

`HttpClient::rebuild_with_options` 构造新的后端和脱敏策略快照，同时复制原客户端注册的请求头注入器、请求拦截器和响应拦截器。包括校验失败在内，原客户端始终不变；`to_builder` 则只复制选项。调用方若依赖运行时注册项，应选择保留它们的重建 API。

默认的 `SameOrigin` 策略会以配置的 base URL 校验绝对请求目标，并以初始源校验每次重定向。未配置 base URL 时，可由绝对请求 URL 确定本次请求的初始源。只有显式选择 `AnyOrigin` 才允许跨源目标及重定向。配置读取器接受 `origin_policy = "same_origin"` 或 `"any_origin"`，不支持的值会在构造客户端前报错。

## 请求与重试边界

`HttpClient::execute` 先解析请求的重试选项。每次获准的尝试都会检查取消状态、运行请求拦截器、解析 URL 和最终请求头、记录请求日志、发送请求、将非 2xx 状态映射为 `HttpError`，最后运行响应拦截器并记录响应日志。响应返回前这一整段路径产生的错误可参与重试；`HttpResponse` 返回后的 body 或 SSE 读取错误由调用方处理，不进入普通 HTTP 重试。

每次重试都会克隆请求。缓冲请求体可以重放；延迟生成的流式请求体由工厂为每次尝试重新创建。原始请求决定是否准入重试流程；流程内则依据拦截器修改后的每次请求重新计算续试资格和 `Retry-After` 偏好。外层尝试次数和时长预算保持固定。`max_duration` 控制是否继续尝试，并非已经开始的请求的硬截止时间；response-header 超时从异步请求准备开始，累计至收到响应头；同步 request interceptor 在期限开始前执行。连接、读取和整次请求超时仍分别配置。最终 `HttpError` 保留重试诊断信息和完整的 `RetryError<HttpError>` 错误来源链。

取消状态会在发送前、受支持的 I/O 阶段以及重试等待期间检查。尝试上下文负责令牌归属：拦截器替换或移除取消令牌后，该次尝试按新状态执行，成功响应也不会错误地恢复旧令牌。SSE 重连中，原始 token 控制重连等待和后续准入；拦截器替换的独立 token 只控制该次 I/O，移除它不会移除流级 token。

## 响应体所有权

`HttpResponse` 的 body 分为后端响应、已缓冲字节和已移交给流三种状态。`bytes`、`text`、`json` 在 `response_body_size_limit` 内聚合数据，成功后缓存；已缓冲响应可以重复读取，也可以重复创建流。尚未缓冲时调用 `stream` 会把后端 body 移交给返回的流，后续聚合读取不能再取回。读取失败会被记录，后续读取仍报告原先的错误类别。流式读取不受整包聚合上限约束；SSE 行/帧和 JSON 解码各自使用相应限额。

非 2xx 响应会在返回调用方前转换为错误。错误 body 的脱敏预览和保留的原始内容分别受独立限额约束，完整原始 body 不会仅因预览截断而标记为不完整。TRACE 日志只记录已经缓存的 body，不会为了日志消费尚未读取的后端流。请求、响应及错误诊断均使用客户端捕获的脱敏策略快照。

## SSE 解码与自动重连

`HttpResponse::sse_messages` 和 `sse_chunks` 是响应体解码器：只有空行才会派发 SSE data 事件，EOF 不派发未完成的数据事件；完整的 retry 与 control-only id 控制记录仍按既有规则处理。仅首行开头的一个 UTF-8 BOM 会被移除。默认 `AllowEof` 允许完整事件流在没有 done marker 时结束，但不派发未完成帧；`RequireDoneMarker` 只接受完整事件中的 marker。解码器执行行/帧限制，以及 JSON chunk 的 JSON 和完成条件限制，但不检查 `Content-Type`。如果上游协议要求 SSE，直接调用者须先校验 `text/event-stream`；`HttpClient::execute_sse_with_reconnect` 会自行校验媒体类型。

重连执行器禁用内层普通 HTTP 重试，打开响应、解码 SSE record、保存最近的事件 ID 和服务端 `retry:` 延迟，然后依照 `SseReconnectOptions` 再次建立连接。有事件 ID 时，后续请求携带 `Last-Event-ID`；未收到新 ID 的消息继承最近游标，control-only ID 更新以及显式空 ID 重置也会保留；空 ID 重置后，最终请求会省略 `Last-Event-ID`，包括默认 header 和注入器写入的值。最新有效的 `retry:` 会持续生效，直到被新有效值替换；`honor_server_retry = false` 会忽略服务端提示。204 响应会正常结束重连流，不检查媒体类型、不读取 body，也不再发起连接。这些是有意的行为变化，不代表完整实现 EventSource。取消与协议错误默认不会重连。重连预算覆盖尝试和等待；最终延迟上限及 SSE 的一毫秒最小等待由重连策略处理。

对于可重连的 429/5xx 握手错误，是否把解析出的 HTTP 提示交给本次重连退避策略，由最终请求的 `honor_retry_after` 偏好决定。HTTP `Retry-After` 仍是只作用于单次尝试的提示，遵守独立的退避策略上限和重连预算；`server_retry_max_delay` 只限制 SSE `retry:` 延迟。SSE 解码错误会从响应补齐缺失的 method、原始请求 URL 和 status，同时保留更具体的已有上下文、脱敏规则和 source。

请求构造不会把原始相对路径带入 URL 解析错误。显式代理 URL 由校验后的域名或 IP 字面量和独立配置的 port 结构化生成，避免 `proxy.host` 混入 authority 分隔符或凭据。multipart 解析会拒绝重复 `boundary`。`stream_body` 调用时会把有限迭代器完整收集到内存；需要延迟创建每次尝试的流时使用 `streaming_body`。

## 修改设计时需要验证的不变量

- 响应已返回后发生的错误不能触发普通 HTTP 重试。
- body 首次读取失败后，后续读取仍保留原错误类别。
- 重试不能重复使用已消费的一次性上传流。
- 拦截器替换或移除取消令牌后的状态只属于对应尝试。
- 直接 SSE 解码与自动重连保持各自的媒体类型契约。
- 日志和错误格式化使用捕获的脱敏策略，并限制 body 预览大小。
- 客户端重建后保留运行时注册项，且不修改原客户端。
- 同源重定向不会连接跨源目标；IPv4-only 客户端也会以 `InvalidUrl` 拒绝 IPv6 字面量重定向，同时仍遵守次数上限。
