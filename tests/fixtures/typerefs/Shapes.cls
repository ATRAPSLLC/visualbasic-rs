VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Shapes"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = True
Option Explicit

' Members, parameters and return values typed by an interface of another
' library (VBA's Collection, stdole's StdFont, IUnknown) and by a public
' UDT, and an event with an interface-typed parameter.

Public Type Point
    X As Long
    Y As Long
End Type

Public Event Moved(ByVal Where As Collection)

Public Items As Collection
Public Auto As New Collection
Public Font As StdFont
Public Unk As IUnknown

Public Sub Pass(ByVal c As Collection, ByRef f As StdFont, p As Point)
    p.X = c.Count
End Sub

Public Function Make(ByVal X As Long) As Point
    Make.X = X
End Function

Public Function Fonts(ByVal n As Long) As StdFont
    Set Fonts = Font
End Function

Public Function NewItems() As Collection
    Set NewItems = New Collection
    RaiseEvent Moved(NewItems)
End Function
