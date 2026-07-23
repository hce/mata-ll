{-# LANGUAGE GHC2021 #-}

-- MllShim: the single, shared GHC-side shim for the differential oracle
-- (see regenerate-ghc-goldens.sh). It supplies the mata-ll Prelude names
-- that GHC's Prelude lacks or types differently, so a test case can run
-- under GHC unchanged. Everything here mirrors a documented mata-ll
-- Prelude/builtin signature; no test-case logic lives in this module.
--
-- Contents:
--   * Number        — mata-ll's floating type (Lua number) = Double.
--   * assert        — mata-ll's test helper: "." per pass, error on fail.
--   * length, take, drop, replicate, (!!)
--                   — mata-ll and GHC both type these with Int (mata-ll's
--                     integer type is `Int`; there is no arbitrary-precision
--                     Integer). The shim exists to pin the same monomorphic
--                     shapes the mata-ll Prelude gives them.
--   * (==) (/=) (<) (<=) (>) (>=) `elem`
--                   — same semantics and fixity (infix 4) as Prelude; only
--                     re-exported because the Int-typed shims above hide
--                     the Prelude names wholesale. mata-ll enforces
--                     Haskell's non-associative precedence-4 grammar itself
--                     (`a == b == c` is a parse error on both sides), so
--                     the old infixl 4 compatibility fixity is gone.
--   * (<$>), (<*>)  — same semantics and fixity (infixl 4) as Prelude.
--                     mata-ll's Prelude fixity declarations now cross
--                     module boundaries, so these bind at infixl 4 in
--                     test-case code exactly as under GHC.
--   * when          — in mata-ll's Prelude; GHC has it in Control.Monad.
--   * getArgs       — mata-ll builtin; GHC has it in System.Environment.
--   * Multiplicity(..) — puts One/Many in scope for `%Many ->` arrows
--                     (GHC.Exts), matching mata-ll's builtin multiplicities.
--   * try, catch    — mata-ll's builtin exception helpers: try yields
--                     Left <message> for any raised error, catch feeds the
--                     message to a handler. Implemented over SomeException;
--                     the corpus never asserts on the message text (the
--                     Lua-side text is host-specific).
--   * read_Integer, read_Number, read_Bool, read_String
--                   — mata-ll's monomorphic read helpers.
--   * ST, runST, STArray and the STArray operations
--                   — mata-ll's builtin mutable Int array
--                     (newSTArray size init, 0-based indices).
module MllShim
  ( Number
  , assert
  , length, take, drop, replicate, (!!)
  , (==), (/=), (<), (<=), (>), (>=), elem
  , (<$>), (<*>)
  , when
  , getArgs
  , Multiplicity (..)
  , try, catch
  , read_Int, read_Number, read_Bool, read_String
  , ST, runST, STArray
  , newSTArray, readSTArray, writeSTArray, modifySTArray
  , stArrayLength, newSTArrayFromList, stArrayToList
  ) where

import Prelude hiding
  ( length, take, drop, replicate, (!!)
  , (==), (/=), (<), (<=), (>), (>=), elem, (<$>), (<*>) )
import qualified Prelude as P
import Control.Monad (when)
import qualified Control.Exception as E
import Control.Monad.ST (ST, runST)
import GHC.Exts (Multiplicity (..))
import System.Environment (getArgs)
import Data.STRef (STRef, newSTRef, readSTRef, modifySTRef')
import qualified Data.Map.Strict as Map

type Number = Double

-- mata-ll: assert True prints ".", assert False raises the message.
assert :: Bool -> String -> IO ()
assert True  _   = putStrLn "."
assert False msg = error msg

-- Int-typed list primitives (mata-ll's integer type is Int; there is no
-- arbitrary-precision Integer). These coincide with GHC's own Int-typed
-- length/take/drop/replicate/(!!), so the shims are plain aliases.
length :: Foldable t => t a -> Int
length = P.length

take :: Int -> [a] -> [a]
take = P.take

drop :: Int -> [a] -> [a]
drop = P.drop

replicate :: Int -> a -> [a]
replicate = P.replicate

(!!) :: [a] -> Int -> a
(!!) = (P.!!)

-- Comparison operators at the shared GHC/mata-ll fixity (infix 4,
-- non-associative — mata-ll enforces the same rejection rule now).
infix 4 ==, /=, <, <=, >, >=
infix 4 `elem`

(==), (/=) :: Eq a => a -> a -> Bool
(==) = (P.==)
(/=) = (P./=)

(<), (<=), (>), (>=) :: Ord a => a -> a -> Bool
(<)  = (P.<)
(<=) = (P.<=)
(>)  = (P.>)
(>=) = (P.>=)

elem :: (Eq a, Foldable t) => a -> t a -> Bool
elem = P.elem

-- Functor/Applicative operators at the shared GHC/mata-ll fixity: the
-- Prelude's infixl 4 declarations now reach every mata-ll module.
infixl 4 <$>, <*>

(<$>) :: Functor f => (a -> b) -> f a -> f b
(<$>) = (P.<$>)

(<*>) :: Applicative f => f (a -> b) -> f a -> f b
(<*>) = (P.<*>)

-- mata-ll's builtin exception helpers (IO-only).
--   try   :: IO a -> IO (Either String a)
--   catch :: IO a -> (String -> IO a) -> IO a
try :: IO a -> IO (Either String a)
try a = either (Left . show @E.SomeException) Right <$> E.try a

catch :: IO a -> (String -> IO a) -> IO a
catch a h = a `E.catch` \e -> h (show @E.SomeException e)

-- Monomorphic read helpers from mata-ll's Prelude.
read_Int :: String -> Int
read_Int = read

read_Number :: String -> Number
read_Number = read

read_Bool :: String -> Bool
read_Bool s = if s P.== "True" then True else False

read_String :: String -> String
read_String s = s

-- mata-ll's STArray: a mutable, 0-indexed Int array scoped to ST s.
-- Backed here by an STRef over Map Int Int; the corpus only uses
-- small arrays, so the representation cost is irrelevant.
newtype STArray s = STArray (STRef s (Map.Map Int Int))

-- newSTArray size init: indices 0 .. size-1, all set to init.
newSTArray :: Int -> Int -> ST s (STArray s)
newSTArray n x =
  STArray <$> newSTRef (Map.fromList [(i, x) | i <- [0 .. n - 1]])

readSTArray :: STArray s -> Int -> ST s Int
readSTArray (STArray r) i = (Map.! i) <$> readSTRef r

writeSTArray :: STArray s -> Int -> Int -> ST s ()
writeSTArray (STArray r) i v = modifySTRef' r (Map.insert i v)

modifySTArray :: STArray s -> Int -> (Int -> Int) -> ST s ()
modifySTArray (STArray r) i f = modifySTRef' r (Map.adjust f i)

stArrayLength :: STArray s -> ST s Int
stArrayLength (STArray r) = Map.size <$> readSTRef r

newSTArrayFromList :: [Int] -> ST s (STArray s)
newSTArrayFromList xs =
  STArray <$> newSTRef (Map.fromList (P.zip [0 ..] xs))

stArrayToList :: STArray s -> ST s [Int]
stArrayToList (STArray r) = Map.elems <$> readSTRef r
