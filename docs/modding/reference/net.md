# `net`

The `net` namespace connects a mod to the world outside the game: HTTP and HTTPS requests, TCP connections out and in, UDP datagrams, local sockets and pipes that other programs on the computer offer, and opening a web page in the player's browser. Every function except `net.close` needs the `net` permission (list `"net"` in the `permissions` of your mod.toml; the player then allows it). Nothing here waits: each call returns at once, and what comes back (a response, bytes received, a connection closing) is handed to your callback or handler functions in a later frame. Those functions run as ordinary handlers, so they may call `sleep`, and they work in the lobby as well as in the world.

```lua
net.request({ url = "https://example.com/status.json" }, function(res)
  if res.error then
    print("request failed: " .. res.error)
  elseif res.status == 200 then
    local data = json.decode(res.body)
    print("the server says: " .. tostring(data.message))
  else
    print("HTTP status " .. res.status)
  end
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`net.request(spec, callback)`](#netrequest) | Starts an HTTP or HTTPS request; the answer goes to `callback` later. |
| [`net.open_url(url)`](#netopen_url) | Opens a web page in the player's browser. |
| [`net.connect(host, port, handlers)`](#netconnect) | Opens a TCP connection to another program. |
| [`net.pipe(name, handlers)`](#netpipe) | Connects to a local socket (Linux, macOS) or named pipe (Windows) another program offers. |
| [`net.listen(port, handlers, options?)`](#netlisten) | Accepts TCP connections from other programs. |
| [`net.udp(port?, handlers, options?)`](#netudp) | Opens a UDP socket to send and receive datagrams. |
| [`net.send(id, data, host?, port?)`](#netsend) | Sends bytes on a connection, or a datagram from a UDP socket. |
| [`net.close(id)`](#netclose) | Closes a connection, pipe, listener or UDP socket. |

## Handlers and ids

`net.connect`, `net.pipe`, `net.listen` and `net.udp` each take a **handlers** table: a plain table of functions, all of them optional, that the client calls when something happens on the socket. Each call returns an **id** (an integer) that names the socket in `net.send`, `net.close` and in every handler call. The handlers run in the mod phase of a later frame, never inside the call that opened the socket.

| Handler | Arguments | Called |
| --- | --- | --- |
| `open` | `id` | Once a connection or pipe is made (`net.connect`, `net.pipe`). |
| `accept` | `listener, id, peer_ip, peer_port` | For each new connection on a listener (`net.listen`). |
| `data` | `id, bytes` (UDP: `id, bytes, from_ip, from_port`) | For each piece of data received. `bytes` is a Lua string of raw bytes. |
| `close` | `id, err` | Once, when a connection, pipe or listener ends by itself. `err` is `nil` when the other side closed it, otherwise a message saying why (it could not connect, the connection broke...). |

TCP connections and pipes are streams: the data does not keep the boundaries of the other side's writes. One message may arrive in several `data` calls, or several messages in one. Mark the end of each message yourself (a newline, or a length in front) and keep what you have received in a buffer until a whole message is there. Each `data` call carries at most 64 KB.

Sockets are closed when the mod unloads.

## `net.request`

```lua
net.request(spec, callback) -> integer
```

Starts an HTTP or HTTPS request on a background thread and returns at once. When the request ends, in a later frame, `callback` is called with one response table. Use it to fetch data from a web service or to post data to one.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `spec` | `table` | yes | What to request; its fields are below. |
| `callback` | `function(res)` | yes | Called once with the response table when the request ends. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `url` | `string` | (required) | The address. It must start with `http://` or `https://`. |
| `method` | `string` | `"GET"` | Any HTTP method (`"GET"`, `"POST"`, `"PUT"`, `"PATCH"`, `"DELETE"`, `"HEAD"`, `"OPTIONS"`...), in any case. |
| `body` | `string` | none | The bytes to send, with whatever method is given. |
| `headers` | `table` | none | Request headers, as `{ ["Name"] = "value" }`. |
| `timeout` | `number` | `30` | Seconds the whole request may take, kept between 1 and 300. |

**Returns** `integer`: an id for this request. The callback is not given it, and a request cannot be cancelled; keep the id only if you want to tell your requests apart.

The callback's `res` table has either the first three fields or `error`:

| Field | Type | Description |
| --- | --- | --- |
| `status` | `integer` | The HTTP status code: 200, 404, 500... Error statuses arrive here, not as `error`. |
| `body` | `string` | The response body as raw bytes (it may be binary). |
| `headers` | `table` | The response headers, their names in lower case: `res.headers["content-type"]`. When a header comes more than once, the last one is kept. |
| `error` | `string` | Set only when the request failed (the address did not resolve, no connection, the timeout ran out, a body of 16 MB or more). The other fields are then `nil`. |

**Needs** the `net` permission.

**Errors** when `url` is missing or does not start with `http://` or `https://` (`"ftp://x": only http and https`); `"<method>" is not an HTTP method` for a method name with spaces or other characters a method cannot have.

**Example**

```lua
net.request({
  url = "https://example.com/api/notes",
  method = "POST",
  headers = { ["Content-Type"] = "application/json" },
  body = json.encode({ text = "hello", time = os.time() }),
  timeout = 10,
}, function(res)
  if res.error then
    print("failed: " .. res.error)
  else
    print("status " .. res.status .. ", " .. #res.body .. " bytes back")
  end
end)
```

The callback runs as a normal handler: it may sleep and read game state (in the world). Several requests run at the same time; their callbacks come in the order they finish.

## `net.open_url`

```lua
net.open_url(url)
```

Opens an `http://` or `https://` address in the player's web browser. Use it for a help page or a link the player asked for.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `url` | `string` | yes | The address to open. |

**Returns** nothing.

**Needs** the `net` permission.

**Errors** when `url` does not start with `http://` or `https://`, and `open_url: <reason>` when the system's browser launcher cannot be started.

**Example**

```lua
commands.register("websearch", function(...)
  local words = table.concat({ ... }, "+")
  net.open_url("https://example.com/search?q=" .. words)
end)
```

The call returns as soon as the browser launcher has started; it does not tell whether a page opened.

## `net.connect`

```lua
net.connect(host, port, handlers) -> integer
```

Opens a TCP connection to `host` and `port`, for example to a companion program or a local server. It returns the connection's id at once and connects in the background; `open` runs when it is connected, `close` (with an error) when it could not connect.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `host` | `string` | yes | A host name or an IP address: `"127.0.0.1"`, `"localhost"`, `"example.com"`. |
| `port` | `integer` | yes | The port, 1..65535. |
| `handlers` | `table` | yes | The handler functions, all optional. |

**`handlers` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `open` | `function(id)` | none | The connection is made. |
| `data` | `function(id, bytes)` | none | Bytes arrived (a string of up to 64 KB). |
| `close` | `function(id, err)` | none | The connection ended: `err` is `nil` when the other side closed it, a message when it failed or could not be made. |

**Returns** `integer`: the connection's id, for `net.send` and `net.close`.

**Needs** the `net` permission.

**Example**

```lua
local buffer = ""
net.connect("127.0.0.1", 7000, {
  open = function(id) net.send(id, "hello\n") end,
  data = function(id, bytes)
    buffer = buffer .. bytes
    for line in buffer:gmatch("([^\n]*)\n") do print("got: " .. line) end
    buffer = buffer:match("[^\n]*$")
  end,
  close = function(id, err) print("closed: " .. (err or "by the other side")) end,
})
```

Connecting gives up after 10 seconds. Bytes you send before `open` runs are kept and sent once the connection is made. After `close` has run, the id no longer works.

## `net.pipe`

```lua
net.pipe(name, handlers) -> integer
```

Connects to a local endpoint that another program on this computer offers, such as a desktop app's control socket. On Linux and macOS it is a Unix socket in the user's runtime folder (`$XDG_RUNTIME_DIR`, else `$TMPDIR`, else `/tmp`): `name` is its path inside that folder, such as `discord-ipc-0` or `app/com.discordapp.Discord/discord-ipc-0`. On Windows it is the named pipe `\\.\pipe\<name>`. The system's control sockets, which would let a mod run programs (the session bus, systemd, the gpg, ssh and keyring agents, docker and podman), are refused.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The socket's path inside the runtime folder (Windows: the pipe's name). No absolute path, no `..`, no `\`. |
| `handlers` | `table` | yes | `open`, `data` and `close`, as for [`net.connect`](#netconnect). |

**Returns** `integer`: the pipe's id, for `net.send` and `net.close`.

**Needs** the `net` permission.

**Errors** `net.pipe "<name>": a name in the runtime folder, not a path out of it` for an absolute path, `..` or `\`; `net.pipe "<name>": a system control socket, refused`; on Windows, `net.pipe "<name>": a pipe name has no folders` for a name with `/`.

**Example**

```lua
net.pipe("my-helper.sock", {
  open = function(id) net.send(id, "status\n") end,
  data = function(id, bytes) print("helper: " .. bytes) end,
  close = function(id, err) print("helper gone", err) end,
})
```

When the endpoint cannot be opened, `close` runs with `"<name>: <reason>"`.

## `net.listen`

```lua
net.listen(port, handlers, options?) -> integer, integer
```

Starts a TCP server that other programs can connect to. The port is taken right away (an error is raised if it is in use); connections are accepted in the background, and each one gets its id and calls the same handlers.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `port` | `integer` | yes | The port to listen on; `0` picks a free one. |
| `handlers` | `table` | yes | The handler functions, all optional. |
| `options` | `table` | no | `{ host = "..." }`: the address to listen on. |

**`handlers` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `accept` | `function(listener, id, peer_ip, peer_port)` | none | A program connected: the listener's id, the new connection's id, and the other side's IP address (a string) and port. |
| `data` | `function(id, bytes)` | none | Bytes arrived on one of the connections; `id` says which. |
| `close` | `function(id, err)` | none | A connection ended (`err` is `nil` when the other side closed it), or the listener itself stopped on an error (then `id` is the listener's). |

`open` is not called for accepted connections: `accept` takes its place.

**`options` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `host` | `string` | `"127.0.0.1"` | The address to listen on. The default only takes connections from this computer; `"0.0.0.0"` takes them from the network too. |

**Returns** `integer, integer`: the listener's id, and the port it listens on (the one picked when you passed `0`).

**Needs** the `net` permission.

**Errors** `net.listen <host>:<port>: <reason>` when the port cannot be taken (in use, not allowed), and `<host>:<port>: <reason>` when `host` is not a valid address.

**Example**

```lua
local listener, port = net.listen(0, {
  accept = function(listener, id, ip, peer_port)
    print("connection " .. id .. " from " .. ip .. ":" .. peer_port)
  end,
  data = function(id, bytes) net.send(id, bytes) end,  -- echo it back
  close = function(id, err) print("connection " .. id .. " closed") end,
})
print("echo server on port " .. port)
```

Closing the listener with `net.close` stops new connections; the ones already accepted stay open until each is closed.

## `net.udp`

```lua
net.udp(port?, handlers, options?) -> integer, integer
```

Opens a UDP socket. It receives the datagrams sent to its port and sends datagrams with `net.send(id, data, host, port)`. Use it for small messages to and from a local program.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `port` | `integer` | no, default `0` | The port to receive on; `nil` or `0` picks a free one. |
| `handlers` | `table` | yes | `{ data = function(id, bytes, from_ip, from_port) }`. |
| `options` | `table` | no | `{ host = "..." }`, as for [`net.listen`](#netlisten): `"127.0.0.1"` by default. |

**`handlers` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `data` | `function(id, bytes, from_ip, from_port)` | none | A datagram arrived: its bytes (one whole datagram per call), and the sender's IP address (a string) and port. |

A UDP socket has no `open` or `close` handler.

**Returns** `integer, integer`: the socket's id and its port.

**Needs** the `net` permission.

**Errors** `net.udp <host>:<port>: <reason>` when the port cannot be taken, and `<host>:<port>: <reason>` when `host` is not a valid address.

**Example**

```lua
local sock, port = net.udp(nil, {
  data = function(id, bytes, ip, from_port)
    print(("%d bytes from %s:%d"):format(#bytes, ip, from_port))
  end,
})
net.send(sock, "ping", "127.0.0.1", 9999)
```

## `net.send`

```lua
net.send(id, data, host?, port?)
```

Sends bytes. On a connection made by `net.connect`, `net.pipe` or `net.listen`, `data` is queued and written in the background, in order. On a UDP socket, `data` goes out as one datagram to `host` and `port` right away.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | A connection's or UDP socket's id. |
| `data` | `string` | yes | The bytes to send (any bytes, not only text). |
| `host` | `string` | UDP only | Where the datagram goes. |
| `port` | `integer` | UDP only | The port it goes to. |

**Returns** nothing.

**Needs** the `net` permission.

**Errors**
- `no socket <id>`: no open socket has this id (it was closed, or never existed).
- `the connection is closed`: the connection has ended.
- `send on a connection, not its listener`: `id` is a listener; send on the connection ids that `accept` gave.
- `a UDP send takes (id, data, host, port)`: a UDP socket was given no `host` or `port`.
- For UDP, an error when `host` is not a valid address or the system refuses the datagram.

**Example**

```lua
local sock = net.udp(nil, {})
-- one datagram for a local program listening on port 5000
net.send(sock, json.encode({ event = "zone", zone = game.zone() }), "127.0.0.1", 5000)
```

On a connection, `net.send` returns before the bytes are written.

## `net.close`

```lua
net.close(id)
```

Closes a connection, pipe, listener or UDP socket (and also a device opened with [`devices.open`](devices.md#devicesopen)). Its handlers are let go: nothing more arrives for it, and its `close` handler is **not** called.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The id `net.connect`, `net.pipe`, `net.listen`, `net.udp` or an `accept` handler gave. |

**Returns** nothing. An id that is not open is ignored.

**Example**

```lua
local conn = net.connect("127.0.0.1", 7000, {})
timer.after(60, function()
  net.close(conn)  -- give up after a minute
end)
```

Closing a listener does not close the connections it accepted.
