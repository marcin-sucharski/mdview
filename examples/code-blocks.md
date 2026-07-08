# Code Block Highlighting

JSON blocks highlight object keys, strings, numbers, and literals.

```json
{
  "ok": true,
  "count": 2,
  "items": ["alpha", "beta"]
}
```

HTTP blocks highlight the request or response line, headers, and the body using
the declared content type.

```http
POST /items HTTP/1.1
Host: api.example.test
Content-Type: application/json

{
  "name": "alpha"
}
```

Request and response examples can be separated with `>>>`.

```http
GET /items/1 HTTP/1.1
Accept: application/json

>>>
HTTP 200 OK
Content-Type: application/json

{
  "id": 1,
  "name": "alpha"
}
```

XML and plain text are supported too.

```xml
<item id="1">alpha</item>
```

```text
plain text stays quiet
```
