data Color = Red | Green | Blue
    deriving Show

data Secret = Secret Int

main :: IO ()
main = do
    -- This should work: Color has Show
    assert (show Red == "Red") "show with deriving"
    -- This should work: Int has Show
    assert (show 42 == "42") "show Int"
    assert (show True == "True") "show Bool"
