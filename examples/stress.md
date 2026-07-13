# Scrolling Stress Fixture

This document is intentionally long enough to exercise continuous scrolling in
tmux. It mixes lorem ipsum prose, tables, HTTP, JSON, PostgreSQL, XML, quotes,
and lists so redraw paths cover the main renderer surfaces.

## Lorem Section 01

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum imperdiet
tellus at lorem facilisis, vitae facilisis risus tincidunt. Integer dictum
aliquet massa, vitae volutpat ipsum aliquet sed.

| Item | Description | Count |
| --- | --- | ---: |
| alpha | Lorem ipsum dolor sit amet, consectetur adipiscing elit. | 12 |
| beta | Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. | 34 |
| gamma | Ut enim ad minim veniam, quis nostrud exercitation ullamco. | 56 |

```json
{
  "section": 1,
  "enabled": true,
  "items": ["alpha", "beta", "gamma"]
}
```

## Lorem Section 02

> Curabitur blandit tempus porttitor. Donec sed odio dui. Etiam porta sem
> malesuada magna mollis euismod.

- Lorem ipsum dolor sit amet
- Consectetur adipiscing elit
- Integer posuere erat a ante

```http
GET /stress/2
Authorization: Bearer ...

200 OK
Content-Type: application/json

{
  "ok": true,
  "page": 2
}
```

## Lorem Section 03

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Praesent commodo cursus
magna, vel scelerisque nisl consectetur et. Cras mattis consectetur purus sit
amet fermentum.

```postgresql
-- Section query
SELECT u.id, u.name::text, $1::uuid, now()
FROM "user" AS u
WHERE u.profile @> '{"role":"admin"}'::jsonb
RETURNING jsonb_build_object('id', u.id);
```

## Lorem Section 04

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Aenean lacinia bibendum
nulla sed consectetur. Maecenas faucibus mollis interdum.

| Region | Status | Notes |
| --- | --- | --- |
| north | active | Nulla vitae elit libero, a pharetra augue. |
| south | paused | Donec ullamcorper nulla non metus auctor fringilla. |
| west | active | Morbi leo risus, porta ac consectetur ac, vestibulum. |

```xml
<items>
  <item id="4">lorem ipsum</item>
</items>
```

## Lorem Section 05

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Donec id elit non mi
porta gravida at eget metus. Nulla vitae elit libero, a pharetra augue.

```text
plain text block
lorem ipsum dolor sit amet
consectetur adipiscing elit
```

## Lorem Section 06

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Duis mollis, est non
commodo luctus, nisi erat porttitor ligula, eget lacinia odio sem nec elit.

| Metric | Value | Trend |
| --- | ---: | --- |
| requests | 128 | up |
| errors | 3 | flat |
| latency | 42ms | down |

## Lorem Section 07

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed posuere consectetur
est at lobortis. Cras justo odio, dapibus ac facilisis in, egestas eget quam.

```json
{
  "section": 7,
  "message": "lorem ipsum",
  "count": 700
}
```

## Lorem Section 08

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Fusce dapibus, tellus
ac cursus commodo, tortor mauris condimentum nibh, ut fermentum massa justo sit
amet risus.

```http
# response comment
POST /stress/8
Content-Type: application/json

{
  "name": "lorem"
}

>>>
HTTP 201 Created
Content-Type: application/json

{
  "id": 8
}
```

## Lorem Section 09

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Integer posuere erat a
ante venenatis dapibus posuere velit aliquet.

| Column A | Column B | Column C |
| --- | --- | --- |
| lorem | ipsum | dolor |
| sit | amet | consectetur |
| adipiscing | elit | vestibulum |

## Lorem Section 10

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Aenean eu leo quam.
Pellentesque ornare sem lacinia quam venenatis vestibulum.

```postgresql
CREATE FUNCTION touch_user() RETURNS trigger AS $$
BEGIN
  NEW.updated_at := now();
  RETURN NEW;
END
$$ LANGUAGE plpgsql;
```

## Lorem Section 11

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Etiam porta sem
malesuada magna mollis euismod. Maecenas sed diam eget risus varius blandit.

## Lorem Section 12

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Nullam quis risus eget
urna mollis ornare vel eu leo. Donec sed odio dui.

## Lorem Section 13

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vivamus sagittis lacus
vel augue laoreet rutrum faucibus dolor auctor.

## Lorem Section 14

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Morbi leo risus, porta
ac consectetur ac, vestibulum at eros.

## Lorem Section 15

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum id ligula
porta felis euismod semper.

## Lorem Section 16

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Donec ullamcorper nulla
non metus auctor fringilla.

## Lorem Section 17

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Aenean lacinia bibendum
nulla sed consectetur.

## Lorem Section 18

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Integer posuere erat a
ante venenatis dapibus posuere velit aliquet.

## Lorem Section 19

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Praesent commodo cursus
magna, vel scelerisque nisl consectetur et.

## Lorem Section 20

Lorem ipsum dolor sit amet, consectetur adipiscing elit. End of the scrolling
stress fixture.
