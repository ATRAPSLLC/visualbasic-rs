Attribute VB_Name = "Program"
Option Explicit

' Drives the events fixture: a WithEvents listener, calls through an
' implemented interface for each return type, a Friend call, property calls
' on a class.

Private Function ThroughInterface(ByVal m As Measure) As Double
    Dim r As Double
    r = m.Area() + m.Perimeter() + m.Cost() + m.Made() + m.Sides()
    r = r + Len(m.Label()) + m.Weight()
    If m.Valid Then r = r + 1
    m.Resize 2
    ThroughInterface = r + m.Area()
End Function

Sub Main()
    Dim l As Listener, c As Ring, n As Double
    Set l = New Listener
    n = l.Listen(4) + Len(l.Log)
    Set c = New Ring
    c.Radius = 2.5
    n = n + c.Radius + c.Diameter() + ThroughInterface(c)
    Set c = Nothing
    Set l = Nothing
End Sub
