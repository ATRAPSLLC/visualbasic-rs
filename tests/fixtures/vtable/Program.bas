Attribute VB_Name = "Program"
Option Explicit

' Typed call forms: calls through IRaw, an interface from Raw.tlb whose
' methods return values directly instead of an HRESULT (VCall* by return
' type), and Declare'd functions of every return type (ImpAdCall* by return
' type). Nothing here runs: the program only has to compile.

Private Declare Function TickLong Lib "kernel32" Alias "GetTickCount" () As Long
Private Declare Function TickInteger Lib "kernel32" Alias "GetTickCount" () As Integer
Private Declare Function TickByte Lib "kernel32" Alias "GetTickCount" () As Byte
Private Declare Function TickBoolean Lib "kernel32" Alias "GetTickCount" () As Boolean
Private Declare Function TickSingle Lib "kernel32" Alias "GetTickCount" () As Single
Private Declare Function TickDouble Lib "kernel32" Alias "GetTickCount" () As Double
Private Declare Function TickCurrency Lib "kernel32" Alias "GetTickCount" () As Currency
Private Declare Function TickDate Lib "kernel32" Alias "GetTickCount" () As Date
Private Declare Function TickString Lib "kernel32" Alias "GetTickCount" () As String
Private Declare Function TickVariant Lib "kernel32" Alias "GetTickCount" () As Variant
Private Declare Function TickObject Lib "kernel32" Alias "GetTickCount" () As Object
Private Declare Sub TickSub Lib "kernel32" Alias "GetTickCount" ()
Private Declare Function MulDiv Lib "kernel32" (ByVal a As Long, ByVal b As Long, ByVal c As Long) As Long

' One call per IRaw method: each return type, a Sub, arguments of several
' widths, and the one method that does return an HRESULT.
Private Function Vtable(ByVal r As IRaw) As Double
    Dim d As Double, s As String, v As Variant, u As IUnknown, o As IRaw
    d = r.GetByte() + r.GetShort() + r.GetLong()
    d = d + r.GetSingle() + r.GetDouble() + r.GetCurrency() + r.GetDate()
    s = r.GetString()
    If r.GetBool() Then d = d + 1
    v = r.GetVariant()
    Set u = r.GetUnknown()
    Set o = r.GetSelf()
    r.Put 1, 2.5, s
    d = d + r.Combine(1, 2, 3.5, 4.5) + r.Checked(7)
    d = d + o.GetSelf().GetDouble()
    Vtable = d + Len(s) + v
End Function

' One call per Declare.
Private Function Declares() As Double
    Dim d As Double, s As String, v As Variant, o As Object
    d = TickLong() + TickInteger() + TickByte() + TickSingle() + TickDouble()
    d = d + TickCurrency() + TickDate()
    If TickBoolean() Then d = d + 1
    s = TickString()
    v = TickVariant()
    Set o = TickObject()
    TickSub
    d = d + MulDiv(2, 3, 4)
    Declares = d + Len(s) + v
End Function

Sub Main()
    Dim r As IRaw, d As Double
    If r Is Nothing Then Exit Sub
    d = Vtable(r) + Declares()
End Sub
