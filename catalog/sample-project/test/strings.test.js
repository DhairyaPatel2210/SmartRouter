import { test } from "node:test";
import assert from "node:assert/strict";
import { capitalize, truncate } from "../src/strings.js";

test("capitalize", () => {
  assert.equal(capitalize("hello"), "Hello");
  assert.equal(capitalize(""), "");
});

test("truncate", () => {
  assert.equal(truncate("hello", 10), "hello");
  assert.equal(truncate("hello world", 6), "hello…");
});
