VERSION 5.00
Begin VB.UserControl Knob 
   ClientHeight    =   600
   ClientLeft      =   0
   ClientTop       =   0
   ClientWidth     =   1500
   ScaleHeight     =   600
   ScaleWidth      =   1500
   Begin VB.Label Readout 
      Caption         =   "0"
      Height          =   255
      Left            =   0
      TabIndex        =   0
      Top             =   0
      Width           =   1500
   End
End
Attribute VB_Name = "Knob"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = True
Option Explicit

' A public UserControl: a property pair, two events
' (one with arguments), a method, and calls on its built-in members.

Public Event Turned(ByVal Position As Long, ByVal Delta As Double)
Public Event Reset()

Private m_Position As Long

Public Property Get Position() As Long
    Position = m_Position
End Property

Public Property Let Position(ByVal p As Long)
    Dim old As Long
    old = m_Position
    m_Position = p
    PropertyChanged "Position"
    Readout.Caption = CStr(p)
    RaiseEvent Turned(p, p - old)
End Property

Public Sub Zero()
    m_Position = 0
    RaiseEvent Reset
    UserControl.Refresh
End Sub

Private Sub UserControl_Resize()
    Readout.Width = UserControl.ScaleWidth
End Sub

Private Sub UserControl_Click()
    Position = Position + 1
End Sub
