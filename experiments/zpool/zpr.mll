-- zpr: acceptance harness for the ZPool reader.
--
--   mll -r zpr.mll [image ...]
--
-- Opens the pool image(s) (default: /var/tmp/zfs-example.img; pass several
-- paths for a mirror), prints the dataset list and the files in foo/bar,
-- then reconstructs every regular file of foo/bar into /var/tmp/zpr-out/
-- for an external shasum comparison against the originals.

import ZPool (Pool, openPool, listDatasets, listFiles, readPath)
import LIOLinear (hPut, hClose, withOutFile)
import LOS (execute)

main :: IO ()
main = do
    args <- getArgs
    let paths = case args of
            [] -> ["/var/tmp/bar.img"]
            ps -> ps
    r <- openPool paths
    case r of
        Left err -> error ("openPool failed: " <> err)
        Right pool -> do
            ds <- listDatasets pool
            putStrLn ("datasets: " <> show ds)
            files <- listFiles pool "foo/bar/baz"
            putStrLn ("files in foo/bar/baz: " <> show files)
            _ <- execute "mkdir -p /var/tmp/zpr-out"
            mapM_ (dump pool) files

dump :: Pool -> String -> IO ()
dump pool name = do
    r <- readPath pool ("foo/bar/baz", name)
    case r of
        Left err -> error ("readPath " <> name <> " failed: " <> err)
        Right bytes -> do
            -- Files can live at nested paths (e.g. mllc/src/codegen.rs), so
            -- create the parent directory before opening the output for write.
            _ <- execute ("mkdir -p \"$(dirname \"/var/tmp/zpr-out/" <> name <> "\")\"")
            -- The output is written through LIOLinear's %1 handle: the
            -- callback receives the handle linearly, so the checker proves
            -- it is written and closed exactly once — forgetting hClose or
            -- touching the handle after it would not compile.
            res <- withOutFile ("/var/tmp/zpr-out/" <> name) (\h -> do
                h2 <- hPut h (bsToString bytes)
                hClose h2)
            case res of
                Left err -> error ("cannot write output: " <> err)
                Right _ -> putStrLn ("wrote /var/tmp/zpr-out/" <> name
                                     <> " (" <> show (bsLength bytes) <> " bytes)")
