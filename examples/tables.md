# Table Rendering

mdview renders GitHub-style pipe tables with measured terminal-cell widths.

| Feature | Status | Notes |
| :--- | :---: | ---: |
| Left aligned text | ready | normal body cells |
| Centered value | yes | `inline code` works |
| Right aligned number | 12345 | links keep [metadata](https://example.test) |
| Unicode | 界 | wide characters stay aligned |

Narrow terminals wrap long cell content inside the table instead of letting it
spill into neighboring columns.

| Name | Description |
| --- | --- |
| long-cell | This cell intentionally contains a long sentence so the table can be checked in a narrow terminal. |
| short | Compact cells remain padded and aligned. |
