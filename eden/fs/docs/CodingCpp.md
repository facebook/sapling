# EdenFS C++ Coding Conventions

Unless otherwise specified, defer to
[Meta C++ Coding Conventions](/wiki/CppCodingConventions) and
[Meta C++ Style Guidelines](/wiki/CppStyle).

## Passing Parameters

### Light Value Parameters

Lightweight value parameters are passed by value. These types fit in registers.

```cpp
void take(int i, float f, char* p, std::string_view sv);
```

### Light Owning Parameters

Lightweight _owning_ types (e.g. `std::string`, `std::vector`,
`std::shared_ptr`, `folly::File`, `folly::Future`) also fit in registers, but
cannot be cheaply copied.

**Unconditional ownership transfer to callee**: by value

```cpp
void take_ownership(folly::File f);
```

**Temporary borrow**: by const reference

```cpp
void borrow(const folly::File& f);
```

**Conditional ownership transfer**: by rvalue reference

```cpp
void maybe_take_ownership(folly::File&& f);
```

Why? Because passing ownership by value is more general and the temporary can be
constructed directly on the stack: https://xania.org/202101/cpp-by-value-args.

### Heavy Structs and Classes

For large structs and classes, owning or not, pass by const reference or rvalue
reference. The cost of memcpy'ing the value on the stack can be nontrivial.

### Type Parameters

Templates allow you to use perfect forwarding. Always write `T&&` and
`std::forward<T>`.

```
template <typename... T>
void forwarding_call(T&&... args) {
    next_call(std::forward<T>(args)...);
}
```

## Build Times

Forward-declare types when possible. Reducing the header include graph has a
material effect on build times, especially when expensive headers such as
`windows.h` can be avoided.

When possible, define any nontrivial function (or even members, with
[pImpl](https://en.cppreference.com/w/cpp/language/pimpl)) in the .cpp file
instead of the header. Remember that anything in a header is compiled N times,
once for each including source.
