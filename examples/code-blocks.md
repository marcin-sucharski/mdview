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

PostgreSQL SQL blocks highlight comments, keywords, casts, strings, JSONB
operators, and dollar-quoted function bodies.

```postgresql
-- PostgreSQL example
SELECT u.id, u.name::text, $1::uuid, now()
FROM "user" AS u
WHERE u.profile @> '{"role":"admin"}'::jsonb
RETURNING jsonb_build_object('id', u.id);

CREATE FUNCTION touch_user() RETURNS trigger AS $$
BEGIN
  NEW.updated_at := now();
  RETURN NEW;
END
$$ LANGUAGE plpgsql;
```

XML and plain text are supported too.

```xml
<item id="1">alpha</item>
```

```text
plain text stays quiet
```
