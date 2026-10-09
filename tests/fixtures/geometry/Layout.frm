VERSION 5.00
Begin VB.Form Layout 
   Caption         =   "Layout"
   ClientHeight    =   4095
   ClientLeft      =   60
   ClientTop       =   450
   ClientWidth     =   6000
   LinkTopic       =   "Layout"
   ScaleHeight     =   4095
   ScaleWidth      =   6000
   StartUpPosition =   3  'Windows Default
   Begin VB.Timer Ticker 
      Interval        =   1000
      Left            =   5040
      Top             =   3480
   End
   Begin VB.CommandButton Hidden 
      Caption         =   "Hidden"
      Height          =   495
      Left            =   -1200
      TabIndex        =   0
      Top             =   -600
      Width           =   1215
   End
   Begin VB.CommandButton Far 
      Caption         =   "Far"
      Height          =   495
      Left            =   40000
      TabIndex        =   1
      Top             =   36000
      Width           =   33000
   End
   Begin VB.Label Wide 
      Caption         =   "Wide"
      Height          =   255
      Left            =   120
      TabIndex        =   2
      Top             =   120
      Width           =   5775
   End
   Begin VB.Line Diagonal 
      X1              =   120
      X2              =   2400.5
      Y1              =   240.25
      Y2              =   1200
   End
   Begin VB.Shape Box 
      Height          =   735
      Left            =   2640
      Top             =   1320
      Width           =   975
   End
   Begin VB.PictureBox Holder 
      Height          =   1215
      Left            =   120
      ScaleHeight     =   1155
      ScaleWidth      =   2115
      TabIndex        =   3
      Top             =   2400
      Width           =   2175
      Begin VB.TextBox Inner 
         Height          =   285
         Left            =   60
         TabIndex        =   4
         Text            =   "Inner"
         Top             =   60
         Width           =   1935
      End
   End
End
Attribute VB_Name = "Layout"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = False
Option Explicit

' Controls at known positions: negative, beyond 32767, nested in a
' container; a Line with fractional coordinates, a Timer, a Shape.

Private Sub Ticker_Timer()
    Diagonal.X2 = Diagonal.X2 + 1
    Hidden.Left = Far.Left - Wide.Width
End Sub
