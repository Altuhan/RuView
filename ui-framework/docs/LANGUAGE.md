# The Kite `.ui` language

A `.ui` file contains components. The last `export`ed component is the main
one: it gets a generated Rust struct, and `kite preview`/`kite render` show it.

```
import { Card } from "widgets/card.ui";

component Badge inherits Rectangle {          // private helper
    in property <string> label;
    background: #e0e7ff;
    border-radius: 6px;
    Text { text: root.label; }
}

export component App inherits Window {        // main component
    in-out property <string> user: "Ada";
    callback logout;
    ...
}
```

## Lexical notes

- Identifiers may contain `-`: `border-radius`, `request-increase`. Write
  subtraction with spaces: `a - b`. (`a-b` is one identifier.) In generated
  Rust, `-` becomes `_` (`request-increase` → `on_request_increase`).
- Comments: `// line` and `/* block */`.
- Numbers: `42` (int), `1.5` (float), `10px` (length), `50%`.
- Colors: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`.
- Strings: `"text"` with escapes `\n \t \" \\` and interpolation `\{expr}`.

## Components and elements

```
export component Name inherits Base { body }
```

`Base` is a built-in element (`Window`, `Rectangle`, ...) or another
component. Without `inherits`, the component is an invisible `Empty`
container. The body contains, in any order:

| Item | Syntax |
|---|---|
| Child element | `Text { ... }` or `id := Text { ... }` |
| Property declaration | `in-out property <int> count: 0;` |
| Binding | `width: 100px;` |
| Two-way binding | `text <=> root.name;` |
| Callback declaration | `callback picked(int, string);` |
| Callback handler | `clicked => { ... }` or `picked(index, label) => { ... }` |
| Children slot | `@children` |

### Ids and name lookup

- `id := Element { }` names an element within its component. Ids must be unique per component.
- `self` is the current element, `parent` its parent, and `root` the component's root element.
- Unqualified names (`count`) resolve to a property of `self`, then of each
  enclosing element in the same component, up to `root`.
- Handler parameters are in scope inside their handler.

### Properties

```
property <type> name;                 // private
in property <type> name: default;     // set from outside, read inside
out property <type> name: binding;    // computed inside, read from outside
in-out property <type> name;          // both
```

Types: `int`, `float`, `length` (same as `float`, logical pixels), `bool`,
`string`, `color` (alias `brush`), and the enums `TextHorizontalAlignment`,
`TextVerticalAlignment` and `LayoutAlignment`.

Rules checked by the compiler:

- `private` properties are visible only inside the declaring component.
- `out` properties cannot be bound or assigned from outside the component.
- Built-in output properties (`pressed`, `has-hover`, `has-focus`, ...) cannot be set.
- `int` converts to `float` implicitly. Going from `float` to `int` needs
  `round()`, `floor()` or `ceil()`. Numbers and bools convert to `string`
  implicitly where a string is expected.

### Bindings and reactivity

A binding is an expression. It is evaluated lazily and re-evaluated when a
property it read changes:

```
out property <string> label: count == 1 ? "1 item" : "\{count} items";
Rectangle { width: parent.width * 50%; background: enabled ? #2563eb : #9ca3af; }
```

Assigning a property in a handler or from Rust replaces its binding, as in
Slint. A two-way binding `a <=> b` makes `a` and `b` the same storage, so
writes through either side are seen by both.

Bindings may read layout results (`x`, `y`, `width`, `height` of any
element). Layout writes these back, and repeats until values settle (at most
4 passes).

### Callbacks

```
callback submitted(string);                 // declare
submitted(text) => { last = text; }         // handle in .ui
Button { clicked => { root.submitted(input.text); } }   // invoke
```

Calling a callback is a statement. It cannot appear inside an expression. A
Rust handler set with `on_submitted` replaces the `.ui` handler.

Handler statements:

```
count = 0;
count += step;               // also -=, *=, /=
if count > 10 { count = 10; } else if count < 0 { count = 0; } else { }
root.changed(count);
```

### Expressions

| Kind | Examples |
|---|---|
| Arithmetic | `a + b`, `a - b`, `a * b`, `a / b` (always float), `a % b` |
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=` (numbers, or strings) |
| Logic | `&&`, `\|\|`, `!x` |
| Conditional | `cond ? a : b` |
| String concat | `"n = " + n` (any operand becomes a string) |
| Interpolation | `"Hello, \{name}!"` |
| Percent | `width: 50%` → `parent.width * 0.5` (for x/y/size properties) |
| Enum value | `horizontal-alignment: center;` or `TextHorizontalAlignment.center` |

Built-in functions (method syntax works too: `c.darker(0.2)`):

| Function | Result |
|---|---|
| `min(a, b)`, `max(a, b)`, `clamp(v, lo, hi)`, `abs(x)` | number (int if all args are int) |
| `round(x)`, `floor(x)`, `ceil(x)` | int |
| `sqrt(x)` | float |
| `rgb(r, g, b)`, `rgba(r, g, b, alpha)` | color (channels 0–255, alpha 0–1) |
| `darker(c, f)`, `brighter(c, f)`, `fade(c, f)`, `mix(a, b, t)` | color |

## Layout

Children of `HorizontalLayout`/`VerticalLayout` are sized along the main
axis from their constraints:

- **min**: `min-width`/`min-height`, else the content's minimum
- **preferred**: `preferred-width`/`preferred-height`, else the content's size (text, widget size)
- **max**: `max-width`/`max-height`. Buttons, check boxes, sliders and text inputs don't grow vertically.
- **stretch**: `horizontal-stretch`/`vertical-stretch`. Space beyond the
  preferred sizes is shared in proportion to stretch. A layout stretches only
  if one of its children does. With `alignment: stretch` and no stretchable
  child, all children share the space.
- An explicit `width`/`height` fixes the size.

On the cross axis, children fill the layout (within their min/max) and are
centered. `padding`, `spacing` and `alignment` (`stretch`, `start`, `center`,
`end`, `space-between`) apply to the layout.

Outside layouts, elements use their `x`/`y`/`width`/`height`. When unset,
containers (`Rectangle`, `TouchArea`, `Empty`, layouts) fill their parent,
widgets take their preferred size, and the element is centered.

The root element always matches the window size. Its `width`/`height` set
the initial window size.

## Imports and composition

```
import { Card, Badge } from "widgets.ui";   // path relative to this file
```

Only `export`ed components can be imported. A component may contain
`@children` to choose where children written at the use site are inserted.
Otherwise they are appended to its root element.

## Rust API

For each exported component `Foo`, `kite-build` generates:

| Generated | For |
|---|---|
| `Foo::new()`, `Foo::default()` | create an instance |
| `get_<prop>()` | every public property |
| `set_<prop>(value)` | `in` and `in-out` properties |
| `on_<callback>(\|foo, args...\| ...)` | set a Rust handler (gets the component) |
| `invoke_<callback>(args...)` | call a callback |
| `instance()` | the dynamic `ComponentInstance` |
| `.run()` | open a window (`use kite::prelude::*`, `desktop` feature) |

Type mapping: `int`→`i32`, `float`/`length`→`f32`, `bool`→`bool`,
`string`→`String`, `color`→`kite::Color`, enums→`kite::Value`.

The dynamic API (`kite::load("app.ui")` with the `interpreter` feature, or
`kite_compiler::compile_file`) offers the same through `get_property`,
`set_property`, `on_callback` and `invoke`, using `kite::Value`.

## Built-in elements

Generated with `kite elements`:

### Common properties (all elements)

| Property | Type | Default |
|---|---|---|
| `x` | float | 0 |
| `y` | float | 0 |
| `width` | float | 0 |
| `height` | float | 0 |
| `min-width` | float | 0 |
| `min-height` | float | 0 |
| `max-width` | float | unbounded |
| `max-height` | float | unbounded |
| `preferred-width` | float | 0 |
| `preferred-height` | float | 0 |
| `horizontal-stretch` | float | 0 |
| `vertical-stretch` | float | 0 |
| `visible` | bool | true |
| `opacity` | float | 1 |

### `Empty`

No extra properties.

### `Window`

| Property | Type | Default | Notes |
|---|---|---|---|
| `title` | string | "Kite" |  |
| `background` | color | #f5f5f7ff |  |

### `Rectangle`

| Property | Type | Default | Notes |
|---|---|---|---|
| `background` | color | #00000000 |  |
| `border-radius` | float | 0 |  |
| `border-width` | float | 0 |  |
| `border-color` | color | #00000000 |  |
| `clip` | bool | false |  |

### `Text`

| Property | Type | Default | Notes |
|---|---|---|---|
| `text` | string | "" |  |
| `color` | color | #1f1f1fff |  |
| `font-size` | float | 14 |  |
| `horizontal-alignment` | TextHorizontalAlignment | left |  |
| `vertical-alignment` | TextVerticalAlignment | top |  |

### `TouchArea`

| Property | Type | Default | Notes |
|---|---|---|---|
| `enabled` | bool | true |  |
| `pressed` | bool | false | output (read-only) |
| `has-hover` | bool | false | output (read-only) |
| `mouse-x` | float | 0 | output (read-only) |
| `mouse-y` | float | 0 | output (read-only) |

Callback: `clicked()`

### `Button`

| Property | Type | Default | Notes |
|---|---|---|---|
| `text` | string | "" |  |
| `enabled` | bool | true |  |
| `primary` | bool | false |  |
| `font-size` | float | 14 |  |
| `pressed` | bool | false | output (read-only) |
| `has-hover` | bool | false | output (read-only) |
| `has-focus` | bool | false | output (read-only) |

Callback: `clicked()`

### `CheckBox`

| Property | Type | Default | Notes |
|---|---|---|---|
| `text` | string | "" |  |
| `color` | color | #1f1f1fff |  |
| `checked` | bool | false |  |
| `enabled` | bool | true |  |
| `font-size` | float | 14 |  |
| `has-hover` | bool | false | output (read-only) |
| `has-focus` | bool | false | output (read-only) |

Callback: `toggled()`

### `Slider`

| Property | Type | Default | Notes |
|---|---|---|---|
| `value` | float | 0 |  |
| `minimum` | float | 0 |  |
| `maximum` | float | 100 |  |
| `step` | float | 1 |  |
| `enabled` | bool | true |  |
| `pressed` | bool | false | output (read-only) |
| `has-focus` | bool | false | output (read-only) |

Callback: `changed(float)`

### `TextInput`

| Property | Type | Default | Notes |
|---|---|---|---|
| `text` | string | "" |  |
| `placeholder-text` | string | "" |  |
| `font-size` | float | 14 |  |
| `color` | color | #1f1f1fff |  |
| `enabled` | bool | true |  |
| `password` | bool | false |  |
| `has-focus` | bool | false | output (read-only) |

Callback: `edited(string)`

Callback: `accepted(string)`

### `HorizontalLayout`

| Property | Type | Default | Notes |
|---|---|---|---|
| `spacing` | float | 0 |  |
| `padding` | float | 0 |  |
| `alignment` | LayoutAlignment | stretch |  |

### `VerticalLayout`

| Property | Type | Default | Notes |
|---|---|---|---|
| `spacing` | float | 0 |  |
| `padding` | float | 0 |  |
| `alignment` | LayoutAlignment | stretch |  |
