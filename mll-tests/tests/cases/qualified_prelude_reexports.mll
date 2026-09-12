-- A library module's export list may name Prelude functions it re-exports
-- (Data.Maybe's `maybe`, Data.List's `map`/`span`/`scanl`, Control.Monad's
-- `mapM_`/`sequence_`), and GHC resolves them through a qualified alias
-- (`M.maybe`) as well as bare. mata-ll's alias table only knew the names a
-- module's own declarations provide, so `M.maybe` went unbound the moment
-- `maybe` moved from Data.Maybe into the Prelude (2026-09-12) — and `L.span`
-- had never resolved. Both spellings must name the one Prelude definition.
import qualified Data.List as L
import qualified Data.Maybe as M
import qualified Control.Monad as C
import Data.Maybe (maybe, fromMaybe)
import Data.List (scanl, map)

main :: IO ()
main = do
    print (M.maybe 0 (+ 1) (Just (41 :: Int)), M.fromMaybe 7 (Nothing :: Maybe Int), maybe 1 (* 2) (Just (4 :: Int)))
    print (L.span even [2, 4, 5, 6 :: Int], L.map (* 3) [1, 2 :: Int], L.scanl (+) 0 [1, 2, 3 :: Int], L.sortBy compare [3, 1, 2 :: Int])
    print (scanl (-) 10 [1, 2 :: Int], map negate [1, 2 :: Int], fromMaybe 0 (Just (9 :: Int)))
    C.mapM_ print [1, 2 :: Int]
    C.sequence_ [putStrLn "a", putStrLn "b"]
    C.forM_ [3 :: Int] print
    print (L.foldl' (+) 0 [1, 2, 3 :: Int], L.length [1, 2 :: Int], M.isJust (Just ()))
