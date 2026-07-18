-- lua-compat-skip: luajit
-- LIOLinear: the linear (%1) file-handle API. The handle threads through
-- hPut (consume-and-return) and ends in hClose; the content is read back
-- with plain LIO to prove the writes landed in order. The luajit skip is
-- the library's documented interpreter requirement: file:write returns
-- the file handle only on Lua 5.2+ (LuaJIT returns true), and hPut's
-- consume-and-return threading is built on that. The rejection side
-- (leaking the handle, using it after close) lives in the
-- linear_rejects_liolinear_* tests in run_mll.rs.
import LIOLinear (WHandle, openOut, hPut, hClose, withOutFile)
import LIO (fileLines)
import LOS (remove, tmpname)

-- A %1 writer: the handle is linear for the whole body, so this function
-- is compile-time-obligated to close exactly once.
writeTwo :: WHandle %1 -> String -> String -> IO ()
writeTwo h a b = do
    h2 <- hPut h a
    h3 <- hPut h2 b
    hClose h3

main :: IO ()
main = do
    path <- tmpname
    -- openOut entry: the handle is born unrestricted (no linear IO monad),
    -- and becomes linear when handed to the %1 writer.
    r <- openOut path
    case r of
        Left err -> error ("openOut failed: " <> err)
        Right h -> writeTwo h "alpha " "beta"
    ls <- fileLines path
    assert (ls == ["alpha beta"]) "openOut/hPut/hClose thread writes in order"
    -- withOutFile entry: the callback's parameter arrow is %1, so the
    -- lambda binder and every handle threaded from it are checked.
    r2 <- withOutFile path (\h -> do
        h2 <- hPut h "gamma"
        hClose h2)
    case r2 of
        Left err -> error ("withOutFile failed: " <> err)
        Right _ -> pure ()
    ls2 <- fileLines path
    assert (ls2 == ["gamma"]) "withOutFile truncates and rewrites"
    -- withOutFile surfaces the host's open error as Left.
    bad <- withOutFile "/nonexistent-dir-mll/x" (\h -> hClose h)
    case bad of
        Left _ -> putStrLn "open error surfaces as Left"
        Right _ -> error "opening an impossible path must be Left"
    _ <- remove path
    pure ()
-- expect: open error surfaces as Left
