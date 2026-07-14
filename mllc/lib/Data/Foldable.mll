module Data.Foldable
    ( foldr, foldl, foldl'
    , foldMap
    , toList
    , null, length
    , elem
    , sum, product
    , maximum, minimum
    , and, or, any, all
    , concat, concatMap
    , find
    ) where

import Data.List (foldl', find)

-- foldr/foldl are Foldable class methods; foldMap, null, length, elem,
-- sum, product, maximum and minimum are length-generic functions in the
-- auto-imported Prelude. They are re-exported above so GHC-style
-- `import Data.Foldable (...)` selections work. and/or/any/all and
-- concat/concatMap remain list-specific (as noted in HASKDIFF.md).

-- The elements of a Foldable structure as a list, from left to right.
-- Not in the Prelude (GHC's Prelude does not export it either), so
-- programs that define their own toList keep compiling.
toList :: Foldable t => t a -> [a]
toList t = foldr (\x xs -> x : xs) [] t
