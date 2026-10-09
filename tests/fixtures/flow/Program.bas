Attribute VB_Name = "Program"
Option Explicit

' Control flow: For/Next over every counter type, For Each, the Do loops,
' Select Case, GoSub, On GoTo/GoSub, error handling (procedures with an error
' handler are the only ones VB marks statement by statement with LargeBos),
' With, IIf/Choose/Switch and GoTo. Each procedure says what it exercises.

Private Type Point
    X As Long
    Y As Long
End Type

Private m_Log As String

' For/Next on an Integer: no Step, constant negative Step, constant positive
' Step, a variable Step, and Exit For.
Private Function ForInteger(ByVal n As Integer) As Integer
    Dim i As Integer, s As Integer, stp As Integer
    For i = 1 To n
        s = s + i
    Next i
    For i = n To 1 Step -1
        s = s - i
    Next
    For i = 0 To 20 Step 3
        s = s + 1
    Next
    stp = 2
    For i = 0 To n Step stp
        If i > 10 Then Exit For
        s = s + i
    Next
    ForInteger = s
End Function

' For/Next on a Long: the same four shapes.
Private Function ForLong(ByVal n As Long) As Long
    Dim i As Long, s As Long, stp As Long
    For i = 1 To n
        s = s + i
    Next
    For i = n To 1 Step -1
        s = s - i
    Next
    stp = -3
    For i = n To 0 Step stp
        If i < 2 Then Exit For
        s = s + i
    Next
    ForLong = s
End Function

' For/Next on a Single, with a fractional Step.
Private Function ForSingle(ByVal n As Single) As Single
    Dim i As Single, s As Single, stp As Single
    For i = 0 To n
        s = s + i
    Next
    For i = n To 0 Step -0.5
        s = s - i
    Next
    stp = 0.25
    For i = 0 To n Step stp
        s = s + i
    Next
    ForSingle = s
End Function

' For/Next on a Double.
Private Function ForDouble(ByVal n As Double) As Double
    Dim i As Double, s As Double, stp As Double
    For i = 1 To n
        s = s + i
    Next
    For i = n To 1 Step -1.5
        s = s - i
    Next
    stp = 0.125
    For i = 0 To n Step stp
        s = s + i
    Next
    ForDouble = s
End Function

' For/Next on a Currency.
Private Function ForCurrency(ByVal n As Currency) As Currency
    Dim i As Currency, s As Currency, stp As Currency
    For i = 1 To n
        s = s + i
    Next
    For i = n To 1 Step -1
        s = s - i
    Next
    stp = 0.5@
    For i = 0 To n Step stp
        s = s + i
    Next
    ForCurrency = s
End Function

' For/Next on a Byte (unsigned): ascending, a constant Step, a variable Step.
Private Function ForByte(ByVal n As Byte) As Long
    Dim i As Byte, s As Long, stp As Byte
    For i = 0 To n
        s = s + i
    Next
    For i = 0 To n Step 5
        s = s + i
    Next
    stp = 7
    For i = 1 To n Step stp
        s = s + i
    Next
    ForByte = s
End Function

' For/Next on a Date: one day at a time and a variable Step.
Private Function ForDate(ByVal first As Date) As Long
    Dim d As Date, s As Long, stp As Date
    For d = first To first + 10
        s = s + 1
    Next
    stp = 2.5
    For d = first + 10 To first Step -stp
        s = s + 1
    Next
    ForDate = s
End Function

' For/Next on a Variant counter: no Step, a constant Step and a Variant Step.
Private Function ForVariant(ByVal n As Variant) As Variant
    Dim v As Variant, s As Variant, stp As Variant
    s = 0
    For v = 1 To n
        s = s + v
    Next
    For v = n To 1 Step -1
        s = s - v
    Next
    stp = 0.5
    For v = 0 To n Step stp
        If v > 3 Then Exit For
        s = s + v
    Next
    ForVariant = s
End Function

' For Each over a Collection with a Variant and with an Object control
' variable, over a fixed array, a dynamic array and a Variant holding an
' array; Exit For out of a For Each.
Private Function ForEachAll() As Long
    Dim c As Collection, v As Variant, o As Object
    Dim fixed(3) As Long, dyn() As String, holder As Variant
    Dim s As Long
    Set c = New Collection
    c.Add 1
    c.Add "two"
    c.Add New Collection
    For Each v In c
        s = s + 1
    Next
    For Each o In c
        If o Is Nothing Then Exit For
        s = s + 1
    Next
    fixed(1) = 5
    For Each v In fixed
        s = s + v
    Next
    ReDim dyn(2)
    dyn(0) = "a"
    For Each v In dyn
        s = s + Len(v)
        If s > 100 Then Exit For
    Next v
    holder = Array(1, 2, 3)
    For Each v In holder
        s = s + v
    Next
    ForEachAll = s
End Function

' Do While/Until at the top, Loop While/Until at the bottom, an endless Do
' with Exit Do, and While/Wend.
Private Function DoLoops(ByVal n As Long) As Long
    Dim i As Long, s As Long
    Do While i < n
        i = i + 1
    Loop
    Do Until i = 0
        i = i - 1
    Loop
    Do
        i = i + 2
    Loop While i < n
    Do
        i = i - 1
    Loop Until i <= 0
    Do
        s = s + 1
        If s >= n Then Exit Do
    Loop
    While s > 0
        s = s - 3
    Wend
    DoLoops = s + i
End Function

' Select Case on an Integer (single values, a list, a range, Is comparisons,
' Case Else), on a String (values, a range, Is) and on a Variant.
Private Function Selects(ByVal n As Integer, ByVal t As String, ByVal v As Variant) As Long
    Dim s As Long
    Select Case n
        Case 0
            s = 1
        Case 1, 2, 3
            s = 2
        Case 4 To 9
            s = 3
        Case Is > 100
            s = 4
        Case 10 To 20, 30, Is < 0
            s = 5
        Case Else
            s = 6
    End Select
    Select Case t
        Case "a", "b"
            s = s + 10
        Case "c" To "m"
            s = s + 20
        Case Is >= "x"
            s = s + 30
        Case Else
            s = s + 40
    End Select
    Select Case v
        Case 1
            s = s + 100
        Case "one", 2 To 3
            s = s + 200
        Case Is <> 0
            s = s + 300
    End Select
    Selects = s
End Function

' Select Case on a Double, a Long and a Currency.
Private Function SelectNumbers(ByVal d As Double, ByVal l As Long, ByVal c As Currency) As Long
    Dim s As Long
    Select Case d
        Case 0.5
            s = 1
        Case 1 To 2.5
            s = 2
        Case Is > 1000#
            s = 3
    End Select
    Select Case l
        Case 1 To 10
            s = s + 10
        Case 11, 12
            s = s + 20
        Case Else
            s = s + 30
    End Select
    Select Case c
        Case 1.5@
            s = s + 100
        Case Is < 0
            s = s + 200
    End Select
    SelectNumbers = s
End Function

' GoSub/Return, a GoSub nested in a GoSub body, and an early Return.
Private Function GoSubs(ByVal n As Long) As Long
    Dim s As Long
    s = n
    GoSub Twice
    GoSub Twice
    GoSub Outer
    GoSubs = s
    Exit Function
Twice:
    s = s * 2
    Return
Outer:
    s = s + 1
    GoSub Twice
    If s > 1000 Then Return
    s = s + 1
    Return
End Function

' On x GoTo and On x GoSub, and a plain GoTo.
Private Function OnGotos(ByVal k As Integer) As Long
    Dim s As Long
    On k GoSub One, Two, Three
    On k GoTo LabelA, LabelB, LabelC
    s = -1
    GoTo Done
LabelA:
    s = s + 10
    GoTo Done
LabelB:
    s = s + 20
    GoTo Done
LabelC:
    s = s + 30
Done:
    OnGotos = s
    Exit Function
One:
    s = 1
    Return
Two:
    s = 2
    Return
Three:
    s = 3
    Return
End Function

' On Error GoTo a label, Err.Number, Err.Description, Resume Next.
Private Function ErrResumeNext(ByVal d As Long) As Long
    Dim s As Long
    On Error GoTo Handler
    s = 100 \ d
    s = s + 1
    ErrResumeNext = s
    Exit Function
Handler:
    m_Log = m_Log & Err.Description
    s = Err.Number
    Resume Next
End Function

' Resume (retry the failing statement) after fixing the cause.
Private Function ErrResume(ByVal d As Long) As Long
    On Error GoTo Handler
    ErrResume = 100 \ d
    Exit Function
Handler:
    d = 1
    Resume
End Function

' Resume to a label, Err.Raise with all its arguments, Err.Clear.
Private Function ErrResumeLabel() As Long
    Dim s As Long
    On Error GoTo Handler
    Err.Raise vbObjectError + 513, "Flow", "raised", "help.hlp", 42
    s = 1
Recover:
    Err.Clear
    ErrResumeLabel = s
    Exit Function
Handler:
    s = Err.Number - vbObjectError
    Resume Recover
End Function

' On Error Resume Next, testing Err.Number inline, then On Error GoTo 0.
Private Function ErrInline(ByVal d As Long) As Long
    Dim s As Long
    On Error Resume Next
    s = 10 \ d
    If Err.Number <> 0 Then
        s = -Err.Number
        Err.Clear
    End If
    On Error GoTo 0
    ErrInline = s
End Function

' Line numbers and Erl, the Error statement, and Err.Raise of a number only.
Private Function ErrLines(ByVal d As Long) As Long
    Dim s As Long
10  On Error GoTo Handler
20  s = 10 \ d
30  If s > 5 Then Error 5
40  Err.Raise 6
50  ErrLines = s
60  Exit Function
Handler:
    ErrLines = Erl
End Function

' A handler that rethrows to the caller and one that exits through a label.
Private Function ErrNested() As Long
    On Error GoTo Outer
    ErrNested = ErrRethrow()
    Exit Function
Outer:
    ErrNested = Err.Number
End Function

Private Function ErrRethrow() As Long
    On Error GoTo Handler
    Err.Raise 11
    Exit Function
Handler:
    Dim n As Long
    n = Err.Number
    On Error GoTo 0
    Err.Raise n
End Function

' With on an object, on a UDT, nested With, and With on an array element.
Private Function Withs() As Long
    Dim c As Collection, p As Point, pts(2) As Point
    Set c = New Collection
    With c
        .Add 1
        .Add 2, "k"
        Withs = .Count
    End With
    With p
        .X = 3
        .Y = .X * 2
    End With
    With pts(1)
        .X = p.Y
        With c
            .Add pts(1).X
        End With
        .Y = c.Count
    End With
    Withs = Withs + p.X + p.Y + pts(1).Y
End Function

' IIf, Choose and Switch (all return Variants).
Private Function Choices(ByVal n As Long) As String
    Dim v As Variant
    v = IIf(n > 0, "positive", "not positive")
    v = v & Choose(n, "one", "two", "three")
    v = v & Switch(n = 1, "a", n = 2, "b", True, "c")
    Choices = v
End Function

' Boolean operators in conditions: And, Or, Not, Xor, comparison chains, and
' single-line If/Else.
Private Function Conditions(ByVal a As Long, ByVal b As Boolean, ByVal t As String) As Long
    Dim s As Long
    If a > 0 And b Then s = 1 Else s = 2
    If a < 0 Or Not b Then s = s + 10
    If (a = 3) Xor b Then s = s + 100
    If t = "x" Then
        s = s + 1000
    ElseIf t <> "y" Then
        s = s + 2000
    Else
        s = s + 3000
    End If
    Conditions = s
End Function

Sub Main()
    Dim total As Double
    total = ForInteger(10) + ForLong(10) + ForSingle(3) + ForDouble(4)
    total = total + ForCurrency(5) + ForByte(30) + ForDate(Now) + ForVariant(5)
    total = total + ForEachAll() + DoLoops(7)
    total = total + Selects(3, "d", 2) + SelectNumbers(1.5, 11, 1.5@)
    total = total + GoSubs(2) + OnGotos(2)
    total = total + ErrResumeNext(0) + ErrResume(0) + ErrResumeLabel()
    total = total + ErrInline(0) + ErrLines(0) + ErrNested()
    total = total + Withs() + Len(Choices(2)) + Conditions(3, True, "z")
    m_Log = m_Log & CStr(total)
End Sub
