newtype Age = Int

mkAge :: Int -> Age
mkAge x = Age x

getAge :: Age -> Int
getAge (Age x) = x

main :: IO ()
main = do
    assert (getAge (mkAge 42) == 42) "newtype roundtrip"
    assert (getAge (Age 0) == 0) "newtype zero"
