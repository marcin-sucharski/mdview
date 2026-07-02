# mdview Basic Example

This file covers the core strict CommonMark shapes that mdview renders.

## Inline Markup

Plain text can include *emphasis*, **strong text**, `inline code`, and
[links](https://commonmark.org/).

## Lists

- First unordered item
- Second unordered item with enough text to wrap across multiple terminal
  columns when the window is narrow
- Third unordered item

1. First ordered item
2. Second ordered item
3. Third ordered item

## Quote

> Markdown preview should preserve the structure of quotes and keep them easy
> to distinguish from normal body text.

## Code

```rust
fn main() {
    println!("hello from mdview");
}
```

## Rule

---

## Local Image

![Small sample image](assets/sample.png)
