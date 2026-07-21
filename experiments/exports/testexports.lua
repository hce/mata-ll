local exports = require "exports"

-- A value export is a plain marshalled Lua value, read directly (no call).
print(exports.foo)          -- 123

-- A function export is called with its arguments.
print(exports.bar(1000))    -- 1123  (1000 + foo)

-- An IO-action export is called to PERFORM the action.
exports.run()               -- prints 246  (bar 123 = 123 + 123)
