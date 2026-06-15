-- Stress test: deeply nested expressions (case, if, let)

deepIf :: Integer -> Integer
deepIf n =
    if n > 30
    then 130
    else
        if n > 29
        then 129
        else
            if n > 28
            then 128
            else
                if n > 27
                then 127
                else
                    if n > 26
                    then 126
                    else
                        if n > 25
                        then 125
                        else
                            if n > 24
                            then 124
                            else
                                if n > 23
                                then 123
                                else
                                    if n > 22
                                    then 122
                                    else
                                        if n > 21
                                        then 121
                                        else
                                            if n > 20
                                            then 120
                                            else
                                                if n > 19
                                                then 119
                                                else
                                                    if n > 18
                                                    then 118
                                                    else
                                                        if n > 17
                                                        then 117
                                                        else
                                                            if n > 16
                                                            then 116
                                                            else
                                                                if n > 15
                                                                then 115
                                                                else
                                                                    if n > 14
                                                                    then 114
                                                                    else
                                                                        if n > 13
                                                                        then 113
                                                                        else
                                                                            if n > 12
                                                                            then 112
                                                                            else
                                                                                if n > 11
                                                                                then 111
                                                                                else
                                                                                    if n > 10
                                                                                    then 110
                                                                                    else
                                                                                        if n > 9
                                                                                        then 109
                                                                                        else
                                                                                            if n > 8
                                                                                            then 108
                                                                                            else
                                                                                                if n > 7
                                                                                                then 107
                                                                                                else
                                                                                                    if n > 6
                                                                                                    then 106
                                                                                                    else
                                                                                                        if n > 5
                                                                                                        then 105
                                                                                                        else
                                                                                                            if n > 4
                                                                                                            then 104
                                                                                                            else
                                                                                                                if n > 3
                                                                                                                then 103
                                                                                                                else
                                                                                                                    if n > 2
                                                                                                                    then 102
                                                                                                                    else
                                                                                                                        if n > 1
                                                                                                                        then 101
                                                                                                                        else
                                                                                                                            0

deepLet :: Integer -> Integer
deepLet n =
    let a1 = n + 1 in
        let a2 = n + 2 in
            let a3 = n + 3 in
                let a4 = n + 4 in
                    let a5 = n + 5 in
                        let a6 = n + 6 in
                            let a7 = n + 7 in
                                let a8 = n + 8 in
                                    let a9 = n + 9 in
                                        let a10 = n + 10 in
                                            let a11 = n + 11 in
                                                let a12 = n + 12 in
                                                    let a13 = n + 13 in
                                                        let a14 = n + 14 in
                                                            let a15 = n + 15 in
                                                                let a16 = n + 16 in
                                                                    let a17 = n + 17 in
                                                                        let a18 = n + 18 in
                                                                            let a19 = n + 19 in
                                                                                let a20 = n + 20 in
                                                                                    let a21 = n + 21 in
                                                                                        let a22 = n + 22 in
                                                                                            let a23 = n + 23 in
                                                                                                let a24 = n + 24 in
                                                                                                    let a25 = n + 25 in
                                                                                                        a1 + a25

data Wrap = WrapI Integer | WrapB Bool | WrapN
    deriving (Show, Eq)

deepCase :: Integer -> Integer
deepCase n = case n > 0 of
    True -> case n > 10 of
        True -> case n > 20 of
            True -> case n > 30 of
                True -> case n > 40 of
                    True -> case n > 50 of
                        True -> case n > 60 of
                            True -> case n > 70 of
                                True -> case n > 80 of
                                    True -> case n > 90 of
                                        True -> 10
                                        False -> 9
                                    False -> 8
                                False -> 7
                            False -> 6
                        False -> 5
                    False -> 4
                False -> 3
            False -> 2
        False -> 1
    False -> 0

main :: IO ()
main = do
    assert (deepIf 0 == 0) "deepIf 0"
    assert (deepIf 100 == 130) "deepIf 100"
    assert (deepIf 15 == 114) "deepIf 15"
    assert (deepLet 10 == 46) "deepLet 10"
    assert (deepCase 0 == 0) "deepCase 0"
    assert (deepCase 5 == 1) "deepCase 5"
    assert (deepCase 55 == 6) "deepCase 55"
    assert (deepCase 95 == 10) "deepCase 95"
    putStrLn "ok"
