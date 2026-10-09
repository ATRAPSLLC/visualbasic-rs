VERSION 5.00
Begin VB.UserControl Gauge 
   ClientHeight    =   600
   ClientLeft      =   0
   ClientTop       =   0
   ClientWidth     =   1500
   ScaleHeight     =   600
   ScaleWidth      =   1500
   Begin VB.Label Caption1 
      Caption         =   "0"
      Height          =   255
      Left            =   0
      TabIndex        =   0
      Top             =   0
      Width           =   1500
   End
End
Attribute VB_Name = "Gauge"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' A UserControl: a property pair, an event raised with an argument, a
' private Sub called from a property and from event handlers, a constituent
' Label, and drawing on its own surface.

Public Event Changed(ByVal NewValue As Long)

Private m_Value As Long

Public Property Get Value() As Long
    Value = m_Value
End Property

Public Property Let Value(ByVal v As Long)
    m_Value = v
    PropertyChanged "Value"
    Redraw
    RaiseEvent Changed(v)
End Property

' Private: called from Value and from the Resize and Paint handlers.
Private Sub Redraw()
    UserControl.Cls
    UserControl.Line (0, 0)-(ScaleWidth * m_Value / 100, ScaleHeight), &HFF0000, BF
    UserControl.CurrentX = 10
    UserControl.CurrentY = 10
    UserControl.Print CStr(m_Value)
    Caption1.Caption = CStr(m_Value)
    Caption1.Width = UserControl.ScaleWidth
End Sub

Private Sub UserControl_InitProperties()
    m_Value = 50
End Sub

Private Sub UserControl_Resize()
    Redraw
End Sub

Private Sub UserControl_Paint()
    Redraw
End Sub
