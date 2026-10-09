Attribute VB_Name = "Program"
Option Explicit

Private Declare Function GetTickCount Lib "kernel32" () As Long
Private Declare Function lstrlenA Lib "kernel32" (ByVal s As String) As Long

Sub Main()
    Dim a As Long, b As Long, c As Double, s As String, v As Variant
    a = 7
    b = 3
    c = (a * b + a \ b - a Mod b) / 2#
    s = "x" & CStr(a) & "y"
    v = a & s
    v = v & "z"
    Dim f As Integer
    f = FreeFile
    Open "out.txt" For Output As #f
    Print #f, s; c
    Close #f
    a = GetTickCount() + lstrlenA(s)
    Swap a, b
    If a > b Then
        s = Left$(s, 2)
    Else
        s = Mid$(s, 2)
    End If
    Select Case a
        Case 1
            b = 2
        Case 2 To 5
            b = 3
        Case Else
            b = 4
    End Select
End Sub

Private Sub Swap(ByRef x As Long, ByRef y As Long)
    Dim t As Long
    t = x
    x = y
    y = t
End Sub
