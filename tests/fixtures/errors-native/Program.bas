Attribute VB_Name = "Program"
Option Explicit

' Error handling compiled to native code: the unwind record a procedure
' stores in its frame names its handler labels, the address of each
' statement for Resume and Resume Next, and the line number of each
' statement for Erl. Each procedure varies one of their lengths.

Private m_Log As String

' Two handlers, switched between: a handler table of two entries.
Private Function TwoHandlers(ByVal d As Long) As Long
    Dim s As Long
    On Error GoTo First
    s = 10 \ d
    On Error GoTo Second
    s = s + 20 \ (d - 1)
    TwoHandlers = s
    Exit Function
First:
    TwoHandlers = -1
    Exit Function
Second:
    TwoHandlers = -2
End Function

' Three handlers: a table of three entries.
Private Function ThreeHandlers(ByVal d As Long) As Long
    On Error GoTo A
    ThreeHandlers = 1 \ d
    On Error GoTo B
    ThreeHandlers = 2 \ (d - 1)
    On Error GoTo C
    ThreeHandlers = 3 \ (d - 2)
    Exit Function
A:
    ThreeHandlers = -1
    Exit Function
B:
    ThreeHandlers = -2
    Exit Function
C:
    ThreeHandlers = -3
End Function

' Resume Next over an even number of statements.
Private Function ResumeEven(ByVal d As Long) As Long
    Dim s As Long
    On Error Resume Next
    s = 1 \ d
    s = s + 1
    ResumeEven = s
End Function

' Resume Next over an odd number of statements.
Private Function ResumeOdd(ByVal d As Long) As Long
    Dim s As Long
    On Error Resume Next
    s = 1 \ d
    s = s + 1
    s = s * 2
    ResumeOdd = s
End Function

' Line numbers on an odd number of statements, read by Erl.
Private Function LinesOdd(ByVal d As Long) As Long
10  On Error GoTo Handler
20  LinesOdd = 1 \ d
30  Exit Function
Handler:
    LinesOdd = Erl
End Function

' Line numbers on an even number of statements, with Resume Next.
Private Function LinesEven(ByVal d As Long) As Long
    Dim s As Long
100 On Error GoTo Handler
110 s = 1 \ d
120 s = s + 1
130 LinesEven = s
140 Exit Function
Handler:
    s = Erl
    Resume Next
End Function

' A handler, Resume, Resume Next and line numbers in one procedure that also
' frees a String and a Variant.
Private Function Everything(ByVal d As Long) As String
    Dim t As String, v As Variant
1   On Error GoTo Handler
2   t = CStr(10 \ d)
3   v = t & "x"
4   Everything = v
5   Exit Function
Handler:
    If d = 0 Then
        d = 1
        Resume
    End If
    Resume Next
End Function

Sub Main()
    Dim g As Guard
    m_Log = m_Log & TwoHandlers(0) & ThreeHandlers(1) & ResumeEven(0)
    m_Log = m_Log & ResumeOdd(0) & LinesOdd(0) & LinesEven(0) & Everything(0)
    Set g = New Guard
    m_Log = m_Log & g.Divide(1, 0) & g.Retry(0)
End Sub
