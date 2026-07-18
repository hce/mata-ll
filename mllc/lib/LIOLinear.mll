-- LIOLinear: linear (%1) file-handle IO, in the style of linear-base's
-- System.IO.Resource.
--
-- A WHandle is a write-only file handle under the exactly-once discipline:
-- every operation consumes the handle, and every operation except hClose
-- hands back a fresh WHandle wrapping the same underlying file. A
-- well-typed writer therefore threads the handle through each hPut and
-- MUST end in hClose — dropping the handle (a leak) or mentioning it
-- twice (a write/close after close) is a compile error, not a runtime
-- one.
--
-- mata-ll has no linear IO monad, so unlike linear-base the handle is not
-- born linear from a bind: openOut returns it inside an ordinary Either.
-- The discipline binds wherever the handle crosses a %1 arrow — which is
-- every operation here — so the idiomatic entry point is withOutFile,
-- whose callback receives the handle at %1 and is checked end to end.

import LIO (FileHandle, fOpen)

-- The linear write-handle. The payload is the raw LIO handle; unwrapping
-- it in a %1 context makes the payload itself linear (aliases inherit the
-- obligation), so the only way to discharge it is to route it into one of
-- the %1 FFI calls below. That is what keeps the API closed: a caller
-- holding a WHandle at %1 can consume it only through hPut or hClose.
newtype WHandle = WHandle FileHandle

-- Raw FFI, linear on the Lua side. The %1 is the documented FFI trust
-- boundary (CAVEATS): the checker charges the handle once per call and
-- trusts the host to consume it exactly once — which io holds up: :write
-- writes and :close closes, neither retains the handle.
--
-- Lua's file:write(...) returns the file handle itself on success (Lua
-- 5.2+, including the embedded 5.4 runtime — NOT LuaJIT, which returns
-- true), which is exactly the consume-and-return shape linear threading
-- needs: the argument is consumed by the call, and the same file comes
-- back to be re-wrapped for the next step. On write failure Lua raises,
-- which surfaces as a runtime error like every other LuaIO binding.
ffiPut :: FileHandle %1 -> String -> LuaIO ":write" FileHandle
ffiClose :: FileHandle %1 -> LuaIO ":close" ()

-- Open a file for writing ("wb"). Left is the host's open error (e.g.
-- permission denied); Right is the linear handle.
openOut :: String -> IO (Either String WHandle)
openOut path = do
    r <- fOpen path "wb"
    case r of
        Left err -> pure (Left err)
        Right fh -> pure (Right (WHandle fh))

-- Write a string; consumes the handle and returns a fresh one wrapping
-- the same file. The unwrapped payload fh is linear here and is consumed
-- exactly once, by the %1 FFI write; the handle the host returns is
-- re-wrapped as the next WHandle in the thread.
hPut :: WHandle %1 -> String -> IO WHandle
hPut (WHandle fh) s = do
    fh2 <- ffiPut fh s
    pure (WHandle fh2)

-- Close the handle; consumes it for good. This is the only operation
-- that ends the thread, which is what makes a missing hClose a leak the
-- checker reports.
hClose :: WHandle %1 -> IO ()
hClose (WHandle fh) = ffiClose fh

-- Bracket: open path, hand the handle to the callback at %1, wrap the
-- result. This is where the guarantee binds for ordinary callers: the
-- callback's argument arrow is literally %1, so the checker enforces
-- exactly-once consumption — ending in hClose — over the whole callback
-- body. Left is the open error, untouched.
withOutFile :: String -> (WHandle %1 -> IO a) -> IO (Either String a)
withOutFile path k = do
    r <- openOut path
    case r of
        Left err -> pure (Left err)
        Right h -> do
            x <- k h
            pure (Right x)
