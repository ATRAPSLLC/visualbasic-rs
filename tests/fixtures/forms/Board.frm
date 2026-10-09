VERSION 5.00
Begin VB.Form Board 
   Caption         =   "Board"
   ClientHeight    =   3090
   ClientLeft      =   60
   ClientTop       =   450
   ClientWidth     =   4680
   LinkTopic       =   "Board"
   ScaleHeight     =   3090
   ScaleWidth      =   4680
   StartUpPosition =   3  'Windows Default
   Begin FormCode.Gauge Gauge1 
      Height          =   600
      Left            =   120
      TabIndex        =   3
      Top             =   2280
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
   Begin VB.TextBox Text1 
      Height          =   375
      Left            =   120
      TabIndex        =   2
      Text            =   "Text1"
      Top             =   1680
      Width           =   2175
   End
   Begin VB.CommandButton Command1 
      Caption         =   "One"
      Height          =   495
      Index           =   1
      Left            =   1560
      TabIndex        =   1
      Top             =   960
      Width           =   1215
   End
   Begin VB.CommandButton Command1 
      Caption         =   "Zero"
      Height          =   495
      Index           =   0
      Left            =   120
      TabIndex        =   0
      Top             =   960
      Width           =   1215
   End
   Begin VB.Label Label1 
      Caption         =   "Label1"
      Height          =   375
      Left            =   120
      TabIndex        =   4
      Top             =   240
      Width           =   2175
   End
End
Attribute VB_Name = "Board"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = False
Option Explicit

' Form code: private Subs and a private Function called from event handlers
' and from each other (the form's private vtable slots), public methods and a
' property pair called from a module, Me, a control array (design-time
' elements, one loaded at run time, Count/UBound, the Index argument), a
' UserControl's property and event, and the Controls collection.

Private m_Clicks As Long

' Private: called from Bump and from Reset.
Private Sub ShowCount()
    Me.Caption = "Clicks: " & m_Clicks
    Label1.Caption = CStr(m_Clicks)
End Sub

' Private: called from the control array's Click handler.
Private Sub Bump(ByVal n As Long)
    m_Clicks = m_Clicks + n
    ShowCount
End Sub

' Private with a result.
Private Function Twice(ByVal n As Long) As Long
    Twice = n * 2
End Function

' Public: called from the module.
Public Sub Reset(ByVal start As Long)
    m_Clicks = start
    ShowCount
    Gauge1.Value = start
End Sub

Public Function Clicks() As Long
    Clicks = m_Clicks
End Function

Public Property Get Title() As String
    Title = Me.Caption
End Property

Public Property Let Title(ByVal s As String)
    Me.Caption = s
    Text1.Text = s
End Property

' A control array's event: the Index argument, an element by index.
Private Sub Command1_Click(Index As Integer)
    Bump Twice(Index + 1)
    Command1(Index).Caption = "#" & Index
    Gauge1.Value = Gauge1.Value + Index
End Sub

' The UserControl's event, with its argument.
Private Sub Gauge1_Changed(ByVal NewValue As Long)
    Label1.Caption = "Gauge " & NewValue
End Sub

Private Sub Form_Load()
    Dim c As Control, n As Long
    Load Command1(2)
    Command1(2).Top = Command1(1).Top + Command1(1).Height
    Command1(2).Visible = True
    n = Command1.Count + Command1.UBound + Command1.LBound
    For Each c In Me.Controls
        If TypeOf c Is CommandButton Then n = n + 1
    Next
    Me.Width = Me.Width + n
    Bump n
End Sub

Private Sub Form_Unload(Cancel As Integer)
    Unload Command1(2)
    Cancel = 0
End Sub
