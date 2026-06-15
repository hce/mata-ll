-- Tests for import hiding syntax

import ExportHelper hiding (publicFn)

-- Define our own publicFn, which would conflict without hiding
publicFn :: Integer -> Integer
publicFn x = x * 100

main :: IO ()
main = do
    -- PublicType constructors are still accessible
    assert (PubA == PubA) "hiding: PubA accessible"
    assert (PubB 5 == PubB 5) "hiding: PubB accessible"

    -- Our local publicFn is used instead of the hidden import
    assert (publicFn 3 == 300) "hiding: local publicFn"
