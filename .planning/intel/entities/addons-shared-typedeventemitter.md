---
path: /Users/jonas/code/sapling/addons/shared/TypedEventEmitter.ts
type: util
updated: 2026-01-21
status: active
---

# TypedEventEmitter.ts

## Purpose

Type-safe event emitter wrapper around EventTarget. Provides compile-time type checking for event listeners with support for data payloads and error events.

## Exports

- `TypedEventEmitter` - Type-safe event emitter class

## Dependencies

None

## Used By

TBD

## Notes

Bridges the gap between JavaScript's EventTarget (untyped) and TypeScript by providing generics for event and data types. Works in both browser and Node.js.
