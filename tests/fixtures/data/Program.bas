Attribute VB_Name = "Program"
Option Explicit

' Data: arrays (static, dynamic, multi-dimensional, of UDTs), Variant
' operators, Currency and Date arithmetic, the string functions and the Mid
' statement, every conversion function, math, fixed-length strings, file I/O,
' Debug.Print, and the type tests. Each procedure says what it exercises.

Private Type Record
    Id As Long
    Code As String * 8
    Score As Double
    Flags(3) As Byte
End Type

Private Type Pair
    Name As String
    Value As Variant
End Type

Private m_Static(1 To 10) As Integer
Private m_Grid() As Double
Private m_Text As String

' ReDim, ReDim Preserve, a multi-dimensional ReDim and its bounds, Erase of a
' dynamic and of a static array, LBound/UBound with a dimension argument.
Private Function Arrays(ByVal n As Long) As Long
    Dim a() As Long, b(2, 3) As String, i As Long, j As Long, s As Long
    Dim fixed(5) As Variant
    ReDim a(n)
    For i = 0 To n
        a(i) = i * i
    Next
    ReDim Preserve a(n * 2)
    ReDim m_Grid(1 To 3, 0 To n)
    For i = LBound(m_Grid, 1) To UBound(m_Grid, 1)
        For j = LBound(m_Grid, 2) To UBound(m_Grid, 2)
            m_Grid(i, j) = i + j / 2
        Next
    Next
    ReDim Preserve m_Grid(1 To 3, 0 To n + 1)
    b(1, 2) = "x"
    b(2, 3) = b(1, 2) & "y"
    m_Static(3) = 7
    m_Static(4) = m_Static(3) + 1
    fixed(0) = "v"
    fixed(1) = fixed(0) & fixed(0)
    s = UBound(a) + LBound(a) + UBound(b, 2) + Len(b(2, 3)) + m_Static(4)
    s = s + m_Grid(2, 1) + Len(fixed(1))
    Erase a
    Erase m_Grid
    Erase m_Static
    Erase b
    Arrays = s
End Function

' Arrays of UDTs: a UDT field, a fixed-length string field, an array inside a
' UDT, copying a whole UDT, and a UDT holding a Variant.
Private Function Records() As Long
    Dim r(3) As Record, one As Record, p() As Pair
    r(1).Id = 5
    r(1).Code = "ABC"
    r(1).Score = 2.5
    r(1).Flags(2) = 9
    one = r(1)
    ReDim p(1)
    p(0).Name = "n"
    p(0).Value = one.Score
    p(1) = p(0)
    Records = one.Id + one.Flags(2) + Len(Trim$(one.Code)) + p(1).Value
End Function

' Variant arithmetic, comparison, concatenation, Like, Mod, \, ^, unary minus,
' Not, And/Or/Xor/Eqv/Imp, Null propagation.
Private Function Variants(ByVal a As Variant, ByVal b As Variant) As Variant
    Dim v As Variant, w As Variant
    v = a + b
    v = v - a * b / 2
    v = v \ 3 + v Mod 4 + a ^ 2
    v = -v
    w = a & b
    If a > b Then v = v + 1
    If a = b Or a <> b Then v = v + 2
    If a >= b And a <= b Then v = v + 3
    If w Like "*1*" Then v = v + 4
    w = Not a
    w = (a And b) Or (a Xor b)
    w = (a Eqv b) + (a Imp b)
    w = Null
    w = w + 1
    v = v + IIf(IsNull(w), 1, 0)
    Variants = v
End Function

' Currency and Date arithmetic and the date functions.
Private Function Money(ByVal c As Currency, ByVal d As Date) As Currency
    Dim x As Currency, e As Date, n As Long
    x = c * 3 + 1.25@
    x = x / 4 - c
    x = -x
    e = d + 7
    e = e - 1.5
    n = DateDiff("d", d, e)
    e = DateAdd("m", 1, e)
    n = n + Year(e) + Month(e) + Day(e) + Hour(e) + Minute(e) + Second(e)
    n = n + Weekday(e) + DatePart("q", e)
    e = DateSerial(2000, 1, 2) + TimeSerial(3, 4, 5)
    If e > d Then n = n + 1
    Money = x + n + Int(c) + Fix(c)
End Function

' The string functions and the Mid statement.
Private Function StringFuncs(ByVal s As String) As String
    Dim t As String, parts() As String, n As Long
    t = Mid$(s, 2, 3) & Left$(s, 1) & Right$(s, 2) & Mid$(s, 3)
    Mid$(t, 1, 2) = "ZZ"
    Mid(t, 2) = "y"
    n = InStr(s, "b") + InStr(2, s, "b") + InStr(1, s, "B", vbTextCompare)
    n = n + InStrRev(s, "b") + StrComp(s, t) + StrComp(s, t, vbTextCompare)
    t = Replace(t, "Z", "z")
    parts = Split("a,b,c", ",")
    t = t & Join(parts, ";") & Format(n, "000") & Format$(1.5, "0.00")
    t = UCase$(t) & LCase(t) & Trim$(" x ") & LTrim(" y") & RTrim$("z ")
    n = n + Len(t) + LenB(t) + Asc(t) + AscW(t)
    t = t & Chr$(65) & Chr(66) & ChrW$(67) & String$(3, "-") & String(2, 42) & Space$(2) & Space(1)
    t = t & StrReverse("abc") & StrConv("abc", vbProperCase)
    t = t & Left(s, 1) & Right(s, 1) & Mid(s, 1, 1) & UCase(s) & LCase$(s) & Trim(s)
    If s < t Then n = n + 1
    If s & t = t & s Then n = n + 2
    StringFuncs = t & CStr(n)
End Function

' Every conversion function, from a Variant and from typed values.
Private Function Conversions(ByVal v As Variant, ByVal d As Double, ByVal s As String) As Double
    Dim i As Integer, l As Long, f As Single, r As Double, c As Currency
    Dim dt As Date, t As String, b As Byte, bo As Boolean, w As Variant
    i = CInt(v) + CInt(d) + CInt(s)
    l = CLng(v) + CLng(d) + CLng(s)
    f = CSng(v) + CSng(d) + CSng(s)
    r = CDbl(v) + CDbl(d) + CDbl(s)
    c = CCur(v) + CCur(d) + CCur(s)
    dt = CDate(v) + CDate(d) + CDate("2000-01-01")
    t = CStr(v) & CStr(d) & CStr(i) & CStr(l) & CStr(c) & CStr(dt) & CStr(bo)
    b = CByte(v) + CByte(d) + CByte(s)
    bo = CBool(v) Or CBool(d) Or CBool(s)
    w = CVar(d) + CVar(s)
    r = r + Val(s) + Val(t) + Len(Str(d)) + Len(Str$(l))
    t = t & Hex(l) & Hex$(i) & Oct(l) & Oct$(b)
    r = r + Fix(d) + Int(d) + Abs(d) + Abs(i) + Abs(l) + Sgn(d) + Sgn(i)
    r = r + Fix(v) + Int(v) + Abs(v) + Sgn(v) + CDec(v)
    i = d
    l = f
    f = c
    c = i
    b = l
    Conversions = r + i + l + f + c + dt + b + Len(t) + w
End Function

' Math: Sqr, Sin, Cos, Tan, Atn, Exp, Log, Rnd, Randomize, ^, \, Mod on each
' integer width.
Private Function Maths(ByVal x As Double, ByVal n As Long, ByVal k As Integer) As Double
    Dim r As Double, b As Byte
    Randomize 1
    r = Sqr(x) + Sin(x) + Cos(x) + Tan(x) + Atn(x) + Exp(x) + Log(x) + Rnd + Rnd(1)
    r = r + x ^ 2 + n ^ k
    r = r + (n \ 3) + (n Mod 3) + (k \ 2) + (k Mod 2)
    b = 200
    r = r + (b \ 3) + (b Mod 7)
    r = r + (x \ 2) + (x Mod 2)
    Maths = r
End Function

' Fixed-length strings: a local, assignment truncating and padding, passing
' one to a function, and one inside a UDT.
Private Function FixedStrings(ByVal s As String) As Long
    Dim f As String * 5, g As String * 10, r As Record
    f = s
    g = f & "!"
    r.Code = g
    FixedStrings = Len(f) + Len(RTrim$(g)) + Len(r.Code) + InStr(f, "a")
End Function

' File I/O: FreeFile, Open in each mode, Print # with several items and both
' separators, Write #, Input #, Line Input #, Get/Put of a UDT with and without
' a record number, Seek, EOF, LOF, Loc, Close.
Private Function Files(ByVal path As String) As Long
    Dim f As Integer, s As String, a As Long, b As String, d As Double
    Dim r As Record, n As Long
    f = FreeFile
    Open path For Output As #f
    Print #f, "one"; 2; "three", 4.5
    Print #f, "a", "b";
    Print #f,
    Print #f, Tab(5); "tab"; Spc(2); "spc"
    Write #f, 1, "two", 3.5
    Write #f,
    Close #f
    Open path For Input As #f
    Line Input #f, s
    Input #f, a, b, d
    Do While Not EOF(f)
        Line Input #f, s
        n = n + 1
    Loop
    n = n + LOF(f) + Loc(f)
    Close #f
    Open path For Append As #f
    Print #f, "more"
    Close #f
    Open path For Random As #f Len = Len(r)
    r.Id = 3
    r.Code = "rec"
    Put #f, 1, r
    Put #f, , r
    Get #f, 1, r
    Get #f, , r
    Seek #f, 1
    n = n + Seek(f) + r.Id
    Close #f
    Open path For Binary Access Read Write As #f
    Put #f, 1, a
    Get #f, 1, a
    Put #f, , s
    Get #f, 1, b
    Close
    Kill path
    Files = n + a
End Function

' Debug.Print with several items, separators and a trailing semicolon.
Private Sub Debugging(ByVal s As String, ByVal n As Long)
    Debug.Print s
    Debug.Print s; n, s
    Debug.Print n;
    Debug.Print
End Sub

' Object identity and type tests: Is, Nothing, TypeOf ... Is, IsNothing via
' Is Nothing, IsObject, IsArray, IsNumeric, IsDate, IsMissing, IsNull,
' IsEmpty, VarType, TypeName.
Private Function Types(Optional ByVal opt As Variant) As Long
    Dim a As Item, b As Item, o As Object, v As Variant, arr(1) As Long, n As Long
    Set a = New Item
    Set b = a
    If a Is b Then n = n + 1
    If Not o Is Nothing Then n = n + 2
    Set o = a
    If TypeOf o Is Item Then n = n + 4
    If TypeOf o Is Collection Then n = n + 8
    If IsMissing(opt) Then n = n + 16
    If IsNull(v) Then n = n + 32
    If IsEmpty(v) Then n = n + 64
    v = arr
    If IsArray(v) And IsObject(o) Then n = n + 128
    If IsNumeric("12") And IsDate("2000-01-01") Then n = n + 256
    n = n + VarType(v) + VarType(a.Name) + Len(TypeName(o)) + Len(TypeName(v))
    Set b = Nothing
    Set o = Nothing
    Types = n
End Function

Sub Main()
    Dim n As Double
    n = Arrays(4) + Records() + Variants(3, 4)
    n = n + Money(2.5@, Now) + Len(StringFuncs("abcdef"))
    n = n + Conversions("12", 3.75, "5") + Maths(2, 7, 3) + FixedStrings("abc")
    n = n + Files(Environ$("TEMP") & "\data.tmp")
    Debugging "x", 1
    n = n + Types() + Types(1)
    m_Text = CStr(n)
End Sub
