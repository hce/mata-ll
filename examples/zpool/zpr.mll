-- zpr: acceptance harness for the ZPool reader.
--
--   mll -r zpr.mll [image ...]
--
-- Opens the pool image(s) (default: /var/tmp/zfs-example.img; pass several
-- paths for a mirror), prints the dataset list and the files in foo/bar,
-- then reconstructs every regular file of foo/bar into /var/tmp/zpr-out/
-- for an external shasum comparison against the originals.

import ZPool (Pool, openPool, listDatasets, listFiles, readPath)
import LIO (fOpen, fWrite, fClose)
import LOS (execute)

main :: IO ()
main = do
    args <- getArgs
    let paths = case args of
            [] -> ["/var/tmp/zfs-example.img"]
            ps -> ps
    r <- openPool paths
    case r of
        Left err -> error ("openPool failed: " <> err)
        Right pool -> do
            ds <- listDatasets pool
            putStrLn ("datasets: " <> show ds)
            files <- listFiles pool "foo/bar"
            putStrLn ("files in foo/bar: " <> show files)
            _ <- execute "mkdir -p /var/tmp/zpr-out"
            mapM_ (dump pool) files

dump :: Pool -> String -> IO ()
dump pool name = do
    r <- readPath pool ("foo/bar", name)
    case r of
        Left err -> error ("readPath " <> name <> " failed: " <> err)
        Right bytes -> do
            res <- fOpen ("/var/tmp/zpr-out/" <> name) "wb"
            case res of
                Left err -> error ("cannot write output: " <> err)
                Right h -> do
                    fWrite h (bsToString bytes)
                    fClose h
                    putStrLn ("wrote /var/tmp/zpr-out/" <> name
                              <> " (" <> show (bsLength bytes) <> " bytes)")
