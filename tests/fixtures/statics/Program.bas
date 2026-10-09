Attribute VB_Name = "Program"
Option Explicit

' Storage outside the frame: Static locals, a Static procedure, the module's
' own Private and Public variables (scalars, a String, a UDT, an array), and
' another module's Public variables.

Public Type Rec
    a As Long
    s As String
    d As Double
End Type

Private m_Count As Long
Private m_Name As String
Private m_Rec As Rec
Private m_Arr(3) As Long
Public g_Total As Double

Private Function Counter() As Long
    Static calls As Long
    Static last As String
    Static r As Rec
    calls = calls + 1
    last = last & "x"
    r.a = r.a + calls
    r.d = r.d + 0.5
    Counter = calls + Len(last) + r.a
End Function

Private Static Function Accumulate(ByVal n As Long) As Long
    Dim total As Long
    Dim text As String
    total = total + n
    text = text & CStr(n)
    Accumulate = total + Len(text)
End Function

Private Sub UseModule()
    m_Count = m_Count + 1
    m_Name = m_Name & "y"
    m_Rec.a = m_Count
    m_Rec.s = m_Name
    m_Rec.d = m_Rec.d * 2
    m_Arr(1) = m_Arr(0) + m_Count
    g_Total = g_Total + m_Rec.d
    With m_Rec
        .a = .a + 1
        .s = .s & "z"
    End With
End Sub

Sub Main()
    Dim i As Long, k As Holder
    For i = 1 To 3
        g_Total = g_Total + Counter() + Accumulate(i)
        UseModule
    Next
    Other.o_Value = Other.o_Value + 1
    Other.o_Text = Other.o_Text & "w"
    Set k = New Holder
    k.Touch
    g_Total = g_Total + k.Touch
End Sub
