Attribute VB_Name = "Program"
Option Explicit

Sub Main()
    Dim c As Counter
    Set c = New Counter
    c.Value = 5
    Dim r As Long
    r = c.AddLong(1, 2)
    Dim d As Double
    d = c.ScaleBy(1.5)
    Dim m As Currency
    m = c.Money(3)
    Dim t As String
    Dim v As Variant
    v = c.Describe(42, t)
    Dim p As Integer
    p = c.Pick()
    p = p + c.Pick(3)
    r = r + c.Total(1, 2, 3)
    Dim f As Single
    f = c.Ratio(3)
    Dim b As Byte
    b = c.Small(7)
    r = r + c.Twice() + c.Value
    Set c.Owner = c
    Dim s As Shape
    Dim q As Square
    Set q = New Square
    q.Init 4
    Set s = q
    r = r + s.Area()
    t = t & s.Name
    Set c = c.Self()
End Sub
