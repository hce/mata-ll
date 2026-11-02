-- GHC's Monoid minimal definition is mempty OR mconcat: base defines
-- `mempty = mconcat []` beside `mconcat = foldr mappend mempty`, so an
-- instance that gives mconcat alone has mempty.

newtype Total = Total Int deriving (Show, Eq)

instance Semigroup Total where
    (<>) (Total a) (Total b) = Total (a + b)

instance Monoid Total where
    mconcat ts = Total (sum [n | Total n <- ts])

newtype Peak = Peak Int deriving (Show, Eq)

instance Semigroup Peak where
    (<>) (Peak a) (Peak b) = Peak (max a b)

instance Monoid Peak where
    mempty = Peak 0

main :: IO ()
main = do
    print (mempty :: Total, mconcat [Total 1, Total 2, Total 3], Total 4 <> mempty)
    print (foldMap (\n -> Total (n * n)) [1, 2, 3], mappend (Total 1) (Total 2))
    print (mempty :: Peak, mconcat [Peak 2, Peak 7, Peak 3], Peak 4 <> mempty)
    print (foldMap Peak [1, 9, 2], mappend (Peak 5) (Peak 1))
