-- Multi-line import lists (F5): layout tokens between import-list items are
-- as meaningless as in an export list. The item loop used to reject the
-- shape with "Expected identifier, found start of a new line".
module Main where

import DiamondShared (Item (..),
                      describe,
                      unwrap)
import DiamondEast hiding (
    east)
import DiamondWest (
      west
    )

main :: IO ()
main = print (describe (Item 5) + unwrap (Item 2) + west (Item 4))

-- expect: 12
