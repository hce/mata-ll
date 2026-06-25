10 REM roll two dice a few times using RND
20 FOR I = 1 TO 6
30 LET D1 = INT(RND(1) * 6) + 1
40 LET D2 = INT(RND(1) * 6) + 1
50 PRINT "ROLL"; I; ":"; D1; "+"; D2; "="; D1 + D2
60 NEXT I
70 END
