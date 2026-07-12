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
# request comment
POST /items HTTP/1.1
Host: api.example.test
# header comment
Content-Type: application/json

# JSON body comment
{
  "name": "alpha"
}
```

Responses can follow the request directly, or be separated with `>>>`.

```http
GET /endpoint
Authorization: ...

# response comment
200 OK
Content-Type: application/json

  # response body comment
{
  "id": 1
}
```

XML and plain text are supported too.

```xml
<item id="1">alpha</item>
```

```text
plain text stays quiet
```
