-- `read` at Int, Integer, Number, Bool and String is GHC's grammar
-- (GHC.Read over Text.Read.Lex), read by one shared lexer: surrounding
-- white space, any depth of parentheses with white space inside, a `-`
-- sign (never `+`) before a number lexeme with white space allowed after
-- it, radix prefixes 0x/0o/0b in either case, a fraction only when the
-- dot is followed by a digit and an exponent only when e/E [+-] is
-- followed by a digit, NaN and Infinity at Number only, the identifiers
-- True/False, and a Haskell string literal with lexLitChar's whole escape
-- set at String — and "Prelude.read: no parse" for everything else, with
-- the accepted AND rejected shapes of each type pinned through catch (N7:
-- the five readers were hand-written pattern sets that disagreed with GHC
-- in both directions, and read at String was the identity). The decimal
-- path is correctly rounded at every magnitude, with GHC's placement
-- rules for an exponent beyond the double's range; `read (show x) == x`
-- holds at every type, negative numbers and escapes included; a
-- Read-constrained function reaches the same readers through a
-- dictionary. Int values here stay below 2^53 (read_int_wrap pins the
-- int64 wrap, which a doubles-only host cannot show).
module Main where

tryRead :: String -> (String -> String) -> IO ()
tryRead s f = catch (seq (f s == f s) (putStrLn (show s <> " -> " <> f s))) (\_ -> putStrLn (show s <> " -> no parse"))

asInt :: String -> String
asInt s = show (read s :: Int)

asInteger :: String -> String
asInteger s = show (read s :: Integer)

asNumber :: String -> String
asNumber s = show (read s :: Number)

asBool :: String -> String
asBool s = show (read s :: Bool)

asString :: String -> String
asString s = show (read s :: String)

viaDict :: (Read a, Show a) => (a -> a) -> String -> String
viaDict f s = show (f (read s))

roundTrip :: (Read a, Show a, Eq a) => a -> Bool
roundTrip x = read (show x) == x

main :: IO ()
main = do
    putStrLn "Int"
    mapM_ (\s -> tryRead s asInt)
        [ "42", " -42 ", "- 42", "(5)", "((42))", "( ( 7 ) )", "(-5)", "( - 5 )", "007", "-00"
        , "0x1F", "0X1f", "0o17", "0O17", "0b11", "0B11", "0x1e3", "-0x10", "( - 0b101 )"
        , "9007199254740992", "-9007199254740991", "00000000000000000042", "0x1FFFFFFFFFFFFF"
        , "0o377777777777777777", "0b100000000000000000000000000000000000000000000000000000"
        , "\t\n\r\f\v5", "\160 5 \160", "5 "
        , "+5", "(5", "(5))", "((-5)", "-(5)", "--5", "- - 5", "-+5", "0x", "0X", "0b12", "0o8", "0x1g"
        , "1e3", "1.0", "1_000", "12abc", "", "  ", "()", "( )", "'5'", "NaN", "Infinity", "True"
        , "\"5\"", "5;", "1 2", "(1)(2)", "`5`", "5.", "-", "- "
        ]
    putStrLn "Integer"
    mapM_ (\s -> tryRead s asInteger)
        [ "99999999999999999999", "-99999999999999999999", "(  -  5  )", "- 0x10", "0xFFFFFFFFFFFFFFFFFFFF"
        , "0b1111111111111111111111111111111111111111111111111111111111111111111", "0o17", "0", "-0"
        , "+5", "(5", "1.5", "1e2", "-1e2", "0x", "12abc", ""
        ]
    putStrLn "Number"
    mapM_ (\s -> tryRead s asNumber)
        [ "3.14", "1e3", "1E3", "1e+3", "1.5e-2", "1.0e-2", "1E-3", "0.5e1", "00.5", "0.1", "0.05", "1.50"
        , "100.0", "1234567.0", "12345678.0", "-0", "-0.0", "0.000", "-0x0", "-0b0"
        , "0x10", "0o17", "0b101", "0X1f", "0xFFFFFFFFFFFFFFFFF", "9007199254740993", "9007199254740993.0"
        , "123456789012345678901234567890.0", "1.7976931348623157e308", "1.7976931348623158e308"
        , "1.7976931348623159e308", "4.9406564584124654e-324", "2.4703282292062327e-324", "2.4703282292062328e-324"
        , "1e400", "-1e400", "1e-400", "-1e-400", "1e99999999999999999999", "1e-99999999999999999999"
        , "0e99999999999999999999", "0.0e99999999999999999999", "-0.0e-99999999999999999999", "0e400"
        , "1e1027", "0.001e1030", "0.0000001e-1020", "123e-1027", "1e-00000000000000000000000000000000003"
        , "NaN", "Infinity", "-Infinity", "- NaN", "(NaN)", "(  -  Infinity )", "((2.5))", "- 2.5", "  1.5  "
        , ".5", "5.", "1.e3", "1e", "1e+", "1.5e", "1.5.2", "1e3e4", "1_000.5", "0x1p4", "0x1.8", "0x1e3.0"
        , "infinity", "Infinityx", "NaN1", "nan", "-", "+1.5", "1.5 2", "(1.5", "1.5)", "", "e3", "1,5"
        ]
    putStrLn "Bool"
    mapM_ (\s -> tryRead s asBool)
        [ "True", "False", "  True", "( True ) ", "((False))", "\tFalse\n"
        , "true", "True1", "Truex", "True'", "TrueFalse", "-True", "(True", "True)", "", "T", "False False", "1"
        ]
    putStrLn "String"
    mapM_ (\s -> tryRead s asString)
        [ "\"hi\"", "\"\"", "  (\"hi\")  ", "((\"a\\\"b\"))", "\"a\\nb\"", "\"\\a\\b\\f\\n\\r\\t\\v\\\\\\\"\\'\""
        , "\"\\65\\x41\\o101\\X41\\O101\"", "\"\\0\"", "\"\\0000065\"", "\"\\x0000000041\"", "\"\\12\\&3\""
        , "\"\\SOH\\SO\\SO\\&H\"", "\"\\SOHx\"", "\"\\^A\\^@\\^[\\^\\\\^]\\^^\\^_\\^Z\"", "\"\\DEL\\SP\\NUL\\ESC\\US\""
        , "\"a\\   \\b\"", "\"a\\\n  \\b\"", "\"\\&\"", "\"\\&\\&\"", "\" \\&x\"", "\"\\255\"", "\"\\128\\200\""
        , "\"\\xff\\o377\"", "\"\t\"", "\"a\160b\"", "\"\\b1\"", "\"\\nul\"", "\"\\0x41\"", "\"\\\\65\""
        , "hi", "\"hi", "\"hi\" x", "\"a\" \"b\"", "\"\\q\"", "\"\\x\"", "\"\\o\"", "\"\\o8\"", "\"\\xg\"", "\"\\^a\""
        , "\"\\^?\"", "\"\\^\"", "\"\\So\"", "\"\\Nul\"", "\"\\B1\"", "\"\\ \"", "\"\\", "-\"a\"", "'a'", "\"\\1114112\""
        , "\"\\99999999999999999999\"", "", "(\"a\"", "\"a\")"
        ]
    putStrLn "through a dictionary"
    putStrLn (viaDict (\x -> x + 1) "0x10 " <> " " <> viaDict (\x -> x * (2 :: Integer)) "(-0b11)")
    putStrLn (viaDict (\x -> x / (2 :: Number)) "- 1e1" <> " " <> viaDict not " (False) " <> " " <> viaDict (\x -> x <> "!") "\"\\72i\"")
    r <- try (pure (viaDict (\x -> x + (1 :: Int)) "1e3") >>= \v -> seq (v == v) (print v))
    putStrLn (case r of
        Left _ -> "dictionary read: no parse"
        Right _ -> "dictionary read: parsed")
    putStrLn "round trips"
    print (map roundTrip [0 :: Int, 1, -1, 42, -42, 9007199254740991, -9007199254740991, 4503599627370496])
    print (map roundTrip [0 :: Integer, -1, 10 ^ 30, negate (10 ^ 30), 2 ^ 100, -12345678901234567890])
    print (map roundTrip [0.0 :: Number, 3.14, -3.14, 1.0e-2, 1.5e300, 5.0e-324, 1.0e7, 123456.789, -0.5, 1 / 0, -1 / 0, 9007199254740992])
    print (map roundTrip [True, False])
    print (map roundTrip ["", "hi", "a\nb\t\"\\", "\SOH\SO\&H", "\128\255\200", "\181\&5", "\0\DEL ", "say \"so\""])
    print (show (read (show (-0.0 :: Number)) :: Number), show (read (show (0.0 :: Number)) :: Number))
    print (map (\x -> show (read (show x) :: Number)) [1.0e-2 :: Number, 0.1, 1.0e22, 1.0e21, 123456789.125, 2.5e-5])
