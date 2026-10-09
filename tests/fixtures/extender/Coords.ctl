VERSION 5.00
Begin VB.UserControl Coords 
   ClientHeight    =   600
   ClientLeft      =   0
   ClientTop       =   0
   ClientWidth     =   1500
   ScaleHeight     =   600
   ScaleWidth      =   1500
End
Attribute VB_Name = "Coords"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = True
Option Explicit

' Events whose parameters have each of stdole's coordinate types, by value
' and by reference: the host receives them converted to its scale.

Public Event Pixels(ByVal XPos As OLE_XPOS_PIXELS, ByVal YPos As OLE_YPOS_PIXELS, ByVal XSize As OLE_XSIZE_PIXELS, ByVal YSize As OLE_YSIZE_PIXELS)
Public Event Himetric(ByVal XPos As OLE_XPOS_HIMETRIC, ByVal YPos As OLE_YPOS_HIMETRIC, ByVal XSize As OLE_XSIZE_HIMETRIC, ByVal YSize As OLE_YSIZE_HIMETRIC)
Public Event Container(ByVal XPos As OLE_XPOS_CONTAINER, ByVal YPos As OLE_YPOS_CONTAINER, ByVal XSize As OLE_XSIZE_CONTAINER, ByVal YSize As OLE_YSIZE_CONTAINER)
Public Event Others(ByVal Cancel As OLE_CANCELBOOL, ByVal Exclusive As OLE_OPTEXCLUSIVE, ByVal Default As OLE_ENABLEDEFAULTBOOL, ByVal Color As OLE_COLOR, ByVal Handle As OLE_HANDLE, ByVal Tri As OLE_TRISTATE)
Public Event Variants(ByVal VV As Variant, VR As Variant, ByVal O As Object, ByVal C As Currency, ByVal D As Date)
Public Event Referenced(XPos As OLE_XPOS_PIXELS, ByVal Plain As Long, YSize As OLE_YSIZE_HIMETRIC)

Public Sub Fire()
    Dim x As OLE_XPOS_PIXELS, y As OLE_YSIZE_HIMETRIC
    RaiseEvent Pixels(1, 2, 3, 4)
    RaiseEvent Himetric(5, 6, 7, 8)
    RaiseEvent Container(9, 10, 11, 12)
    RaiseEvent Referenced(x, 13, y)
    Dim vr As Variant
    RaiseEvent Variants(1, vr, Nothing, 2, Now)
    RaiseEvent Others(True, False, True, 0, 0, Gray)
End Sub
